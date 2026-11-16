//! 仅测试使用的依赖装配，避免各模块重复构造真实依赖。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use umt_access_control::AccessControl;
use umt_audit::AuditService;
use umt_core::{
    Connection, ConnectionConfig, ConnectionId, TokenContext, TokenPermission, UmtError, UnixMillis,
};
use umt_persistence::{ConnectionRepo, InMemoryRepos, SecretBlob, SecretRepo};
use umt_protocol_api::MockProvider;
use umt_protocol_registry::ProtocolRegistry;
use umt_security::{AesGcmCipher, SecretCipher, StaticKeyStore, TokenService};
use umt_session_manager::{SessionLimits, SessionManager};

use crate::deps::{ConnectionConfigSource, McpDeps};
use crate::handler::UmtMcpServer;

/// 内存版连接配置来源。
pub(crate) struct MapConfigSource {
    configs: Mutex<BTreeMap<ConnectionId, ConnectionConfig>>,
}

impl MapConfigSource {
    fn new() -> Self {
        Self {
            configs: Mutex::new(BTreeMap::new()),
        }
    }

    fn insert(&self, connection_id: ConnectionId, config: ConnectionConfig) {
        self.configs
            .lock()
            .expect("配置锁被污染")
            .insert(connection_id, config);
    }
}

impl ConnectionConfigSource for MapConfigSource {
    fn get_config(
        &self,
        connection_id: ConnectionId,
    ) -> BoxFuture<'_, Result<Option<ConnectionConfig>, UmtError>> {
        Box::pin(async move {
            Ok(self
                .configs
                .lock()
                .expect("配置锁被污染")
                .get(&connection_id)
                .cloned())
        })
    }
}

/// 真实依赖（内存仓库 + MockProvider）组成的测试环境。
pub(crate) struct TestEnv {
    pub(crate) deps: Arc<McpDeps>,
    pub(crate) repos: InMemoryRepos,
    pub(crate) access: Arc<AccessControl>,
    cipher: Arc<dyn SecretCipher>,
    configs: Arc<MapConfigSource>,
}

impl TestEnv {
    pub(crate) fn new() -> Self {
        let repos = InMemoryRepos::new();
        let access = Arc::new(AccessControl::new(
            repos.tokens.clone(),
            repos.connections.clone(),
            repos.grants.clone(),
            TokenService::from_keystore(&StaticKeyStore::new()).expect("装载 pepper 失败"),
        ));
        let audit = Arc::new(AuditService::new(repos.audit.clone()));
        let sessions = Arc::new(SessionManager::new(
            repos.sessions.clone(),
            audit.clone(),
            SessionLimits::default(),
        ));

        let mut registry = ProtocolRegistry::new();
        registry
            .register(Arc::new(MockProvider::new()))
            .expect("注册 MockProvider 失败");

        let cipher: Arc<dyn SecretCipher> =
            Arc::new(AesGcmCipher::from_keystore(&StaticKeyStore::new()).expect("装载主密钥失败"));
        let configs = Arc::new(MapConfigSource::new());

        let deps = Arc::new(McpDeps {
            protocols: Arc::new(registry),
            access: access.clone(),
            sessions,
            audit,
            connections: repos.connections.clone(),
            secrets: repos.secrets.clone(),
            host_keys: repos.host_keys.clone(),
            cipher: cipher.clone(),
            configs: configs.clone(),
        });

        Self {
            deps,
            repos,
            access,
            cipher,
            configs,
        }
    }

    pub(crate) fn server(&self) -> UmtMcpServer {
        UmtMcpServer::new(self.deps.clone())
    }

    pub(crate) async fn token(&self, name: &str) -> TokenContext {
        let created = self
            .access
            .create_token(name, None, None)
            .await
            .expect("创建 Token 失败");
        self.access
            .authenticate(&created.plaintext, None)
            .await
            .expect("认证失败")
    }

    /// WriteAccess + 交互式终端，用于会话相关测试。
    pub(crate) async fn write_token(&self, name: &str) -> TokenContext {
        let created = self
            .access
            .create_token_with_permissions(name, None, None, TokenPermission::WriteAccess, true)
            .await
            .expect("创建 WriteAccess Token 失败");
        self.access
            .authenticate(&created.plaintext, None)
            .await
            .expect("认证失败")
    }

    pub(crate) async fn connection(&self, name: &str, protocol_id: &str) -> Connection {
        let connection = Connection {
            id: ConnectionId::new(),
            name: name.to_string(),
            protocol_id: protocol_id.to_string(),
            description: None,
            tags: vec![],
            notes: None,
            enabled: true,
            created_at: UnixMillis::now(),
            updated_at: UnixMillis::now(),
            revision: 1,
        };
        self.repos
            .connections
            .create(connection.clone())
            .await
            .expect("创建连接失败");
        connection
    }

    pub(crate) async fn grant(&self, token: &TokenContext, connection_id: ConnectionId) {
        self.access
            .set_grant(token.token_id, connection_id, true)
            .await
            .expect("授权失败");
    }

    pub(crate) fn configure(
        &self,
        connection_id: ConnectionId,
        protocol_id: &str,
        values: serde_json::Value,
    ) {
        self.configs
            .insert(connection_id, ConnectionConfig::new(protocol_id, values));
    }

    pub(crate) async fn store_secret(&self, connection_id: ConnectionId, plaintext: &[u8]) {
        let encrypted = self.cipher.encrypt(plaintext).expect("加密失败");
        self.repos
            .secrets
            .put(
                connection_id,
                SecretBlob {
                    ciphertext: encrypted.ciphertext,
                    nonce: encrypted.nonce.to_vec(),
                    key_version: encrypted.key_version,
                },
            )
            .await
            .expect("写入密文失败");
    }
}
