//! 应用状态：数据库、仓库、安全服务、会话管理与 MCP Server 的装配与托管。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use umt_access_control::AccessControl;
use umt_audit::AuditService;
use umt_core::{ConnectionId, UmtError};
use umt_mcp_server::{McpDeps, McpServer, McpServerConfig};
use umt_persistence::repo::sqlite::SqliteRepos;
use umt_persistence::{ConnectionRepo, Database, HostKeyRepo, SecretRepo, SettingsRepo};
use umt_protocol_registry::ProtocolRegistry;
use umt_provider_ssh::SshProvider;
use umt_provider_telnet::TelnetProvider;
use umt_provider_wsl::WslProvider;
use umt_security::{AesGcmCipher, KeyStore, OsKeyStore, SecretCipher, TokenService};
use umt_session_manager::{CommandPolicy, ExecutionMode, SessionLimits, SessionManager};

use crate::services::secrets::{load_secret_map, store_secret_map};
use crate::services::store::{ConnectionConfigStore, McpConnectionSource};

/// 数据库文件名。旧版 `umt.db` 一律不读取。
pub const DEFAULT_DB_FILE: &str = "umt-v2.db";
/// MCP 默认监听地址。
pub const DEFAULT_MCP_HOST: &str = "127.0.0.1";
/// MCP 默认端口。
pub const DEFAULT_MCP_PORT: u16 = 17321;
/// 默认空闲会话超时（秒）。
pub const DEFAULT_SESSION_TIMEOUT_SECS: u64 = 1800;

/// 设置键：MCP 监听地址。
pub const KEY_MCP_HOST: &str = "mcp.host";
/// 设置键：MCP 端口。
pub const KEY_MCP_PORT: &str = "mcp.port";
/// 设置键：默认执行模式。
pub const KEY_EXECUTION_MODE: &str = "security.default_execution_mode";
/// 设置键：空闲会话超时（秒）。
pub const KEY_SESSION_TIMEOUT: &str = "security.session_timeout_secs";
/// 设置键：高风险操作二次确认。
pub const KEY_REQUIRE_CONFIRMATION: &str = "security.require_confirmation_for_high_risk";
/// 设置键：日志级别。
pub const KEY_LOG_LEVEL: &str = "log.level";

/// 应用数据目录：`%LOCALAPPDATA%/UniversalMCPTerminal`。
pub fn default_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("UniversalMCPTerminal")
}

/// 托管给 Tauri 的全局状态。
pub struct AppState {
    pub data_dir: PathBuf,
    pub repos: SqliteRepos,
    pub pool: SqlitePool,
    pub cipher: Arc<dyn SecretCipher>,
    pub access: Arc<AccessControl>,
    pub audit: Arc<AuditService>,
    pub sessions: Arc<SessionManager>,
    pub protocols: Arc<ProtocolRegistry>,
    pub configs: ConnectionConfigStore,
    pub mcp_deps: Arc<McpDeps>,
    /// MCP 句柄。设置变更或手动启动时会在锁内替换 / 重启。
    pub mcp: tokio::sync::Mutex<McpServer>,
}

impl AppState {
    /// 生产环境初始化：主密钥与 pepper 存放在系统凭据管理器。
    pub async fn initialize(data_dir: impl AsRef<Path>) -> Result<Self, UmtError> {
        let keystore: Arc<dyn KeyStore> = Arc::new(OsKeyStore::default());
        Self::initialize_with_keystore(data_dir, keystore).await
    }

    /// 允许测试注入内存密钥库的初始化入口。
    pub async fn initialize_with_keystore(
        data_dir: impl AsRef<Path>,
        keystore: Arc<dyn KeyStore>,
    ) -> Result<Self, UmtError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)
            .map_err(|err| UmtError::Storage(format!("无法创建数据目录：{err}")))?;

        let database = Database::connect(&data_dir.join(DEFAULT_DB_FILE)).await?;
        let pool = database.pool().clone();
        let repos = SqliteRepos::new(pool.clone());

        let cipher: Arc<dyn SecretCipher> =
            Arc::new(AesGcmCipher::from_keystore(keystore.as_ref())?);
        let token_service = TokenService::from_keystore(keystore.as_ref())?;

        seed_defaults(&repos).await?;

        let access = Arc::new(AccessControl::new(
            repos.tokens.clone(),
            repos.connections.clone(),
            repos.grants.clone(),
            token_service,
        ));
        let audit = Arc::new(AuditService::new(repos.audit.clone()));

        let (policy, limits) = read_security_config(&repos).await?;
        let sessions = Arc::new(SessionManager::new(
            repos.sessions.clone(),
            audit.clone(),
            limits,
        ));
        sessions.set_policy(policy);
        access.set_revoker(sessions.clone());

        let mut registry = ProtocolRegistry::new();
        registry.register(Arc::new(SshProvider::new()))?;
        registry.register(Arc::new(TelnetProvider::new()))?;
        registry.register(Arc::new(WslProvider::new()))?;
        let protocols = Arc::new(registry);

        let configs = ConnectionConfigStore::new(pool.clone());
        let mcp_sources = McpConnectionSource::new(
            configs.clone(),
            repos.secrets.clone() as Arc<dyn SecretRepo>,
            cipher.clone(),
            repos.host_keys.clone() as Arc<dyn HostKeyRepo>,
        );
        let mcp_deps = Arc::new(McpDeps {
            protocols: protocols.clone(),
            access: access.clone(),
            sessions: sessions.clone(),
            audit: audit.clone(),
            connections: repos.connections.clone() as Arc<dyn ConnectionRepo>,
            secrets: repos.secrets.clone() as Arc<dyn SecretRepo>,
            host_keys: repos.host_keys.clone() as Arc<dyn HostKeyRepo>,
            cipher: cipher.clone(),
            configs: Arc::new(mcp_sources),
        });

        let mcp_config = read_mcp_config(&repos).await?;
        let mcp = tokio::sync::Mutex::new(McpServer::new(mcp_config, mcp_deps.clone()));

        Ok(Self {
            data_dir,
            repos,
            pool,
            cipher,
            access,
            audit,
            sessions,
            protocols,
            configs,
            mcp_deps,
            mcp,
        })
    }

    /// 导出目录（不存在时由导出命令创建）。
    pub fn export_dir(&self) -> PathBuf {
        self.data_dir.join("exports")
    }

    /// 读取连接的明文字段映射。
    pub async fn load_secrets(
        &self,
        connection_id: ConnectionId,
    ) -> Result<BTreeMap<String, String>, UmtError> {
        load_secret_map(
            self.repos.secrets.as_ref(),
            self.cipher.as_ref(),
            connection_id,
        )
        .await
    }

    /// 写入或清理连接密文。
    pub async fn store_secrets(
        &self,
        connection_id: ConnectionId,
        secrets: &BTreeMap<String, String>,
    ) -> Result<(), UmtError> {
        store_secret_map(
            self.repos.secrets.as_ref(),
            self.cipher.as_ref(),
            connection_id,
            secrets,
        )
        .await
    }

    /// 连接是否存在密文。
    pub async fn has_secret(&self, connection_id: ConnectionId) -> Result<bool, UmtError> {
        Ok(self.repos.secrets.get(connection_id).await?.is_some())
    }

    pub async fn setting(&self, key: &str) -> Result<Option<String>, UmtError> {
        Ok(self.repos.settings.get(key).await?)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), UmtError> {
        self.repos.settings.set(key, value).await?;
        Ok(())
    }
}

/// 从设置中还原 MCP 监听配置；非法值回退到默认值。
pub async fn read_mcp_config(repos: &SqliteRepos) -> Result<McpServerConfig, UmtError> {
    let host = repos
        .settings
        .get(KEY_MCP_HOST)
        .await?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_MCP_HOST.to_string());
    let port = repos
        .settings
        .get(KEY_MCP_PORT)
        .await?
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_MCP_PORT);
    Ok(McpServerConfig { host, port })
}

/// 读取安全执行策略与 Session 限制；非法值回退到安全默认（Restricted）。
pub async fn read_security_config(
    repos: &SqliteRepos,
) -> Result<(CommandPolicy, SessionLimits), UmtError> {
    let mode = repos
        .settings
        .get(KEY_EXECUTION_MODE)
        .await?
        .and_then(|value| value.trim().parse::<ExecutionMode>().ok())
        .unwrap_or_default();
    let require_confirmation = repos
        .settings
        .get(KEY_REQUIRE_CONFIRMATION)
        .await?
        .map(|value| value.trim() != "false")
        .unwrap_or(true);
    let idle_timeout_secs = repos
        .settings
        .get(KEY_SESSION_TIMEOUT)
        .await?
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_SESSION_TIMEOUT_SECS);

    Ok((
        CommandPolicy {
            mode,
            require_confirmation_for_high_risk: require_confirmation,
            ..CommandPolicy::default()
        },
        SessionLimits {
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            ..SessionLimits::default()
        },
    ))
}

/// 首次启动写入默认设置；已有值绝不覆盖。
async fn seed_defaults(repos: &SqliteRepos) -> Result<(), UmtError> {
    for (key, value) in [
        (KEY_MCP_HOST, DEFAULT_MCP_HOST.to_string()),
        (KEY_MCP_PORT, DEFAULT_MCP_PORT.to_string()),
        (KEY_EXECUTION_MODE, "Restricted".to_string()),
        (
            KEY_SESSION_TIMEOUT,
            DEFAULT_SESSION_TIMEOUT_SECS.to_string(),
        ),
        (KEY_REQUIRE_CONFIRMATION, "true".to_string()),
        (KEY_LOG_LEVEL, "info".to_string()),
    ] {
        if repos.settings.get(key).await?.is_none() {
            repos.settings.set(key, &value).await?;
        }
    }
    Ok(())
}
