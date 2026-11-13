//! `config_json` 读取与 MCP 依赖来源。
//!
//! 持久化层的 `ConnectionRepo` 契约只暴露连接元数据，非敏感协议配置需要直接读
//! `connections.config_json`，因此在这里集中实现，避免 MCP 层接触 SQL。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use umt_core::{ConnectionConfig, ConnectionId, UmtError, UnixMillis};
use umt_mcp_server::ConnectionConfigSource;
use umt_persistence::{HostKeyRepo, SecretRepo};
use umt_protocol_api::SecretValues;
use umt_security::SecretCipher;

use crate::services::secrets::load_secret_map;

/// 连接非敏感配置的 SQLite 读写。
#[derive(Debug, Clone)]
pub struct ConnectionConfigStore {
    pool: SqlitePool,
}

impl ConnectionConfigStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 读取连接配置；连接不存在时返回 `None`。
    pub async fn get(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Option<ConnectionConfig>, UmtError> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT protocol_id, config_json FROM connections WHERE id = ?")
                .bind(connection_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|err| UmtError::Storage(err.to_string()))?;

        let Some((protocol_id, config_json)) = row else {
            return Ok(None);
        };
        let values = serde_json::from_str(&config_json)
            .map_err(|_| UmtError::Storage("连接配置不是合法的 JSON".into()))?;
        Ok(Some(ConnectionConfig::new(protocol_id, values)))
    }

    /// 覆盖连接配置；连接不存在时报 `NotFound`。
    pub async fn set(
        &self,
        connection_id: ConnectionId,
        values: &serde_json::Value,
    ) -> Result<(), UmtError> {
        let config_json = serde_json::to_string(values)
            .map_err(|_| UmtError::Validation("连接配置无法序列化".into()))?;
        let done =
            sqlx::query("UPDATE connections SET config_json = ?, updated_at = ? WHERE id = ?")
                .bind(config_json)
                .bind(UnixMillis::now().0)
                .bind(connection_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(|err| UmtError::Storage(err.to_string()))?;

        if done.rows_affected() == 0 {
            return Err(UmtError::NotFound {
                entity: "连接",
                id: connection_id.to_string(),
            });
        }
        Ok(())
    }
}

/// MCP 运行期连接来源：配置 + 解密后的敏感字段 + 已信任的 Host Key 指纹。
///
/// `McpDeps` 的 `ConnectionConfigSource` 只需要配置；敏感字段与 Host Key 由
/// `McpDeps` 中的 `secrets` / `cipher` / `host_keys` 在调用时读取。本结构额外提供
/// 这两个读取方法，保持「所有 MCP 连接装配逻辑只有一处」。
pub struct McpConnectionSource {
    configs: ConnectionConfigStore,
    secrets: Arc<dyn SecretRepo>,
    cipher: Arc<dyn SecretCipher>,
    host_keys: Arc<dyn HostKeyRepo>,
}

impl McpConnectionSource {
    pub fn new(
        configs: ConnectionConfigStore,
        secrets: Arc<dyn SecretRepo>,
        cipher: Arc<dyn SecretCipher>,
        host_keys: Arc<dyn HostKeyRepo>,
    ) -> Self {
        Self {
            configs,
            secrets,
            cipher,
            host_keys,
        }
    }

    /// 解密 MCP 建立会话所需的敏感字段。
    pub async fn load_secrets(
        &self,
        connection_id: ConnectionId,
    ) -> Result<SecretValues, UmtError> {
        let map: BTreeMap<String, String> =
            load_secret_map(self.secrets.as_ref(), self.cipher.as_ref(), connection_id).await?;
        Ok(SecretValues::new(map))
    }

    /// 已固定的 SSH Host Key 指纹；配置缺少 host/port 时视为未信任。
    pub async fn expected_host_key_fingerprint(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Option<String>, UmtError> {
        let (Some(host), Some(port)) = (config.get_str("host"), config.get_u16("port")) else {
            return Ok(None);
        };
        let endpoint = format!("{host}:{port}");
        Ok(self
            .host_keys
            .get(&endpoint)
            .await?
            .map(|record| record.fingerprint))
    }
}

impl ConnectionConfigSource for McpConnectionSource {
    fn get_config(
        &self,
        connection_id: ConnectionId,
    ) -> BoxFuture<'_, Result<Option<ConnectionConfig>, UmtError>> {
        Box::pin(async move { self.configs.get(connection_id).await })
    }
}
