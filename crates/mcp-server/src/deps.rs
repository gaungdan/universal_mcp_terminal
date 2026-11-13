//! MCP Server 运行期依赖聚合。
//!
//! 所有权限、会话与凭据操作都必须通过这里的依赖完成；MCP 层不直接访问数据库，
//! 也不缓存任何 Token、密码或私钥。

use std::sync::Arc;

use futures::future::BoxFuture;
use umt_access_control::AccessControl;
use umt_audit::AuditService;
use umt_core::{ConnectionConfig, ConnectionId, UmtError};
use umt_persistence::{ConnectionRepo, HostKeyRepo, SecretRepo};
use umt_protocol_registry::ProtocolRegistry;
use umt_security::SecretCipher;
use umt_session_manager::SessionManager;

/// 连接协议配置的来源。
///
/// 持久化层的 `ConnectionRepo` 契约只暴露连接元数据，`config_json` 没有对应的读取方法，
/// 因此由 App 层在启动时注入一个实现（读取同一张表的 `config_json` 即可）。
/// MCP 层只读取配置，从不写入，也不会把配置回显给调用方。
pub trait ConnectionConfigSource: Send + Sync {
    /// 读取连接的非敏感协议配置；不存在返回 `Ok(None)`。
    fn get_config(
        &self,
        connection_id: ConnectionId,
    ) -> BoxFuture<'_, Result<Option<ConnectionConfig>, UmtError>>;
}

/// MCP Server 的全部外部依赖。
pub struct McpDeps {
    pub protocols: Arc<ProtocolRegistry>,
    pub access: Arc<AccessControl>,
    pub sessions: Arc<SessionManager>,
    pub audit: Arc<AuditService>,
    pub connections: Arc<dyn ConnectionRepo>,
    pub secrets: Arc<dyn SecretRepo>,
    pub host_keys: Arc<dyn HostKeyRepo>,
    pub cipher: Arc<dyn SecretCipher>,
    pub configs: Arc<dyn ConnectionConfigSource>,
}
