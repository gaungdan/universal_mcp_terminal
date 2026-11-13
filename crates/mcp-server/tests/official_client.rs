//! 官方 MCP 客户端验收：用 rmcp 自带的 `StreamableHttpClientTransport` 连接本服务，
//! 走完整的 `initialize` 握手、`tools/list` 与 `tools/call` 往返。
//!
//! 装配方式与生产一致：`InMemoryRepos` + 真实 `AccessControl` / `SessionManager` /
//! `AuditService` + `MockProvider`，用 `McpServer` 绑定 `127.0.0.1:0`（系统分配端口）。
//!
//! 全部用例离线运行（仅访问本机回环端口），不依赖真实 SSH / Telnet / WSL。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, JsonObject,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Value, json};
use umt_access_control::AccessControl;
use umt_audit::AuditService;
use umt_core::{Connection, ConnectionConfig, ConnectionId, TokenContext, UmtError, UnixMillis};
use umt_mcp_server::{ConnectionConfigSource, McpDeps, McpServer, McpServerConfig, TOOL_NAMES};
use umt_persistence::{ConnectionRepo, InMemoryRepos};
use umt_protocol_api::MockProvider;
use umt_protocol_registry::ProtocolRegistry;
use umt_security::{AesGcmCipher, SecretCipher, StaticKeyStore, TokenService};
use umt_session_manager::{SessionLimits, SessionManager};

const TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------- 依赖装配

/// 内存版连接配置来源，与 App 层实现同一契约。
struct MapConfigSource {
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
struct Harness {
    repos: InMemoryRepos,
    access: Arc<AccessControl>,
    configs: Arc<MapConfigSource>,
    deps: Arc<McpDeps>,
}

impl Harness {
    fn new() -> Self {
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
            cipher,
            configs: configs.clone(),
        });

        Self {
            repos,
            access,
            configs,
            deps,
        }
    }

    /// 创建 Token，返回其调用上下文与明文（明文只在这里出现）。
    async fn token(&self, name: &str) -> (TokenContext, String) {
        let created = self
            .access
            .create_token(name, None, None)
            .await
            .expect("创建 Token 失败");
        let context = self
            .access
            .authenticate(&created.plaintext, None)
            .await
            .expect("认证失败");
        (context, created.plaintext)
    }

    async fn connection(&self, name: &str) -> Connection {
        let connection = Connection {
            id: ConnectionId::new(),
            name: name.to_string(),
            protocol_id: "mock".into(),
            description: Some(format!("{name} 描述")),
            tags: vec!["official-client".into()],
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

    async fn grant(&self, token: &TokenContext, connection_id: ConnectionId) {
        self.access
            .set_grant(token.token_id, connection_id, true)
            .await
            .expect("授权失败");
    }

    fn configure(&self, connection_id: ConnectionId, values: Value) {
        self.configs
            .insert(connection_id, ConnectionConfig::new("mock", values));
    }

    /// 启动 MCP 服务并返回句柄与 endpoint。
    async fn spawn(&self) -> (McpServer, String) {
        let server = McpServer::new(
            McpServerConfig {
                host: "127.0.0.1".into(),
                port: 0,
            },
            self.deps.clone(),
        );
        let endpoint = server.start().await.expect("MCP 服务启动失败");
        (server, endpoint)
    }
}

// ---------------------------------------------------------------- 客户端辅助

/// 用官方 SDK 建立连接：Bearer Token 通过 transport 的 `auth_header` 注入，
/// 不进 JSON-RPC 参数。
async fn connect(
    endpoint: &str,
    token: &str,
) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ClientConfig>, String> {
    let config =
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token.to_owned());
    let transport = StreamableHttpClientTransport::from_config(config);
    // 显式声明客户端信息，避免依赖 `()` 处理器的隐式默认值。
    let handler = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("umt-official-client", env!("CARGO_PKG_VERSION")),
    );

    tokio::time::timeout(TIMEOUT, handler.serve(transport))
        .await
        .map_err(|_| "官方客户端初始化超时".to_string())?
        .map_err(|error| format!("官方客户端初始化失败：{error:?}"))
}

fn args(value: Value) -> JsonObject {
    value
        .as_object()
        .cloned()
        .expect("工具参数必须是 JSON 对象")
}

fn structured<'a>(result: &'a rmcp::model::CallToolResult, label: &str) -> &'a Value {
    result
        .structured_content
        .as_ref()
        .unwrap_or_else(|| panic!("{label} 缺少 structuredContent"))
}

// -------------------------------------------------------------------- 用例

/// 官方客户端完成 `initialize` 后：`tools/list` 返回 9 个工具；
/// `list_connections` 能看到已授权连接；`open_session` / `close_session` 成功。
#[tokio::test]
async fn official_client_completes_handshake_and_tool_calls() {
    let env = Harness::new();
    let (context, token) = env.token("claude").await;
    let connection = env.connection("official-mock").await;
    env.configure(connection.id, json!({ "host": "local", "mode": "ok" }));
    env.grant(&context, connection.id).await;

    let (server, endpoint) = env.spawn().await;
    let client = connect(&endpoint, &token)
        .await
        .expect("官方客户端应能完成 initialize 握手");

    // 1) tools/list：9 个工具，名称与需求一致。
    let listed = tokio::time::timeout(TIMEOUT, client.list_tools(None))
        .await
        .expect("tools/list 超时")
        .expect("tools/list 失败");
    let mut names: Vec<&str> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    let mut expected = TOOL_NAMES.to_vec();
    names.sort_unstable();
    expected.sort_unstable();
    assert_eq!(names.len(), 9, "官方客户端应看到 9 个工具：{names:?}");
    assert_eq!(names, expected, "工具名称集合与需求不一致");

    // 2) list_connections：能看到被授权的连接。
    let result = tokio::time::timeout(
        TIMEOUT,
        client.call_tool(CallToolRequestParams::new("list_connections")),
    )
    .await
    .expect("list_connections 超时")
    .expect("list_connections 失败");
    let payload = structured(&result, "list_connections");
    let connections = payload["connections"]
        .as_array()
        .expect("缺少 connections 数组");
    assert_eq!(connections.len(), 1, "应只看到已授权连接：{payload}");
    assert_eq!(connections[0]["connection_id"], connection.id.to_string());
    assert_eq!(payload["total"], 1);

    // 3) open_session：MockProvider 上开一个 24x80 的会话。
    let result = tokio::time::timeout(
        TIMEOUT,
        client.call_tool(
            CallToolRequestParams::new("open_session").with_arguments(args(json!({
                "connection_id": connection.id.to_string(),
                "rows": 24,
                "cols": 80,
            }))),
        ),
    )
    .await
    .expect("open_session 超时")
    .expect("open_session 失败");
    let payload = structured(&result, "open_session");
    assert_eq!(payload["connection_id"], connection.id.to_string());
    assert_eq!(payload["rows"], 24);
    assert_eq!(payload["cols"], 80);
    let session_id = payload["session_id"]
        .as_str()
        .expect("open_session 缺少 session_id")
        .to_string();

    // 4) close_session：关闭同一会话。
    let result = tokio::time::timeout(
        TIMEOUT,
        client.call_tool(
            CallToolRequestParams::new("close_session").with_arguments(args(json!({
                "session_id": session_id,
            }))),
        ),
    )
    .await
    .expect("close_session 超时")
    .expect("close_session 失败");
    let payload = structured(&result, "close_session");
    assert_eq!(payload["session_id"], session_id.as_str());

    let mut client = client;
    client.close().await.expect("关闭官方客户端失败");
    server.stop().await.expect("停止服务失败");
}

/// 错误 Token 的官方客户端必须在初始化阶段（或首个请求）失败。
#[tokio::test]
async fn official_client_with_invalid_token_fails() {
    let env = Harness::new();
    let (_context, _token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;

    let error = connect(&endpoint, "umt_00000000_0123456789abcdef0123456789abcdef")
        .await
        .expect_err("非法 Token 必须导致失败");
    assert!(!error.is_empty(), "非法 Token 的失败原因不能为空：{error}");

    server.stop().await.expect("停止服务失败");
}
