//! T20：MCP 端到端与权限矩阵验证。
//!
//! 装配方式与生产一致：`InMemoryRepos` + 真实 `AccessControl` / `SessionManager` /
//! `AuditService` + `MockProvider`，用 `McpServer` 绑定 `127.0.0.1:0`（系统分配端口），
//! 再通过 `reqwest` 发真实 HTTP 请求，走 rmcp 的 Streamable HTTP 传输。
//!
//! 全部用例离线运行，不依赖真实 SSH / Telnet / WSL。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use umt_access_control::AccessControl;
use umt_audit::AuditService;
use umt_core::{
    Connection, ConnectionConfig, ConnectionId, SessionId, TokenContext, TokenStatus, UmtError,
    UnixMillis,
};
use umt_mcp_server::{ConnectionConfigSource, McpDeps, McpServer, McpServerConfig, TOOL_NAMES};
use umt_persistence::{ConnectionRepo, InMemoryRepos, SecretBlob, SecretRepo};
use umt_protocol_api::MockProvider;
use umt_protocol_registry::ProtocolRegistry;
use umt_security::{AesGcmCipher, SecretCipher, StaticKeyStore, TokenService};
use umt_session_manager::{SessionLimits, SessionManager};

const PROTOCOL_VERSION: &str = "2025-03-26";

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
    cipher: Arc<dyn SecretCipher>,
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
            cipher: cipher.clone(),
            configs: configs.clone(),
        });

        Self {
            repos,
            access,
            cipher,
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
            tags: vec!["e2e".into()],
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

    async fn store_secret(&self, connection_id: ConnectionId, plaintext: &[u8]) {
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

// ---------------------------------------------------------------- HTTP 客户端

/// 一次 JSON-RPC 往返的结果。rmcp 3.9 默认使用 SSE（legacy session 模式），
/// 因此响应体可能是单个 JSON，也可能是 `data:` 分帧的多条消息。
#[derive(Debug)]
struct Reply {
    status: u16,
    session_id: Option<String>,
    messages: Vec<Value>,
}

impl Reply {
    fn result(&self) -> Option<&Value> {
        self.messages
            .iter()
            .find_map(|message| message.get("result"))
    }

    fn error(&self) -> Option<&Value> {
        self.messages
            .iter()
            .find_map(|message| message.get("error"))
    }

    /// 错误码来自 `to_mcp_error` 附加的 `data.code`。
    fn error_code(&self) -> Option<&str> {
        self.error()?.get("data")?.get("code")?.as_str()
    }

    /// `tools/call` 的结构化结果：`CallToolResult.structuredContent`。
    fn structured(&self) -> Option<&Value> {
        self.result()?.get("structuredContent")
    }

    /// HTTP 200 且带回 `result` 才算成功。
    fn succeeded(&self) -> bool {
        self.status == 200 && self.result().is_some()
    }

    /// 被拒绝的两种合法形态：HTTP 401（服务层 Bearer 中间件）
    /// 或 JSON-RPC `access_denied`（工具层纵深防御）。
    fn rejected(&self) -> bool {
        self.result().is_none()
            && (self.status == 401 || self.error_code() == Some("access_denied"))
    }
}

fn parse_messages(raw: &str) -> Vec<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        return vec![value];
    }
    raw.lines()
        .filter_map(|line| line.trim().strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .filter(Value::is_object)
        .collect()
}

/// 发送一次 `POST /mcp`。`token = None` 表示不带 `Authorization` 头。
async fn post(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    body: &Value,
) -> Reply {
    let mut request = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .json(body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(session_id) = session_id {
        request = request.header("Mcp-Session-Id", session_id);
    }

    let response = request.send().await.expect("HTTP 请求失败");
    let status = response.status().as_u16();
    let session_id = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let raw = response.text().await.expect("读取响应体失败");
    Reply {
        status,
        session_id,
        messages: parse_messages(&raw),
    }
}

/// 完成 `initialize` 握手后的 MCP 会话。
struct McpClient {
    client: reqwest::Client,
    url: String,
    token: String,
    session_id: String,
    next_id: u64,
}

impl McpClient {
    async fn connect(url: &str, token: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("创建 HTTP 客户端失败");

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "umt-t20", "version": "0.1.0" },
            },
        });
        let reply = post(&client, url, Some(token), None, &initialize).await;
        assert_eq!(reply.status, 200, "initialize 握手失败：{reply:?}");
        let session_id = reply
            .session_id
            .clone()
            .expect("initialize 未返回 Mcp-Session-Id");

        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let ack = post(&client, url, Some(token), Some(&session_id), &initialized).await;
        assert_eq!(
            ack.status, 202,
            "notifications/initialized 未被接受：{ack:?}"
        );

        Self {
            client,
            url: url.to_string(),
            token: token.to_string(),
            session_id,
            next_id: 2,
        }
    }

    /// 使用会话默认 Token 调用。
    async fn request(&mut self, method: &str, params: Value) -> Reply {
        let token = self.token.clone();
        self.request_as(Some(&token), method, params).await
    }

    /// 显式指定（或不指定）Authorization。
    async fn request_as(&mut self, token: Option<&str>, method: &str, params: Value) -> Reply {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        post(
            &self.client,
            &self.url,
            token,
            Some(&self.session_id),
            &body,
        )
        .await
    }

    async fn tool(&mut self, name: &str, arguments: Value) -> Reply {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }
}

// -------------------------------------------------------------------- 用例

/// 未带 Authorization 的 `tools/call` 必须被拒绝（工具层鉴权）。
#[tokio::test]
async fn unauthenticated_tool_call_is_rejected() {
    let env = Harness::new();
    let (_context, token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;

    // 无 Token 的请求会在 HTTP 层被 Bearer 中间件拒绝（401）；
    // 若中间件被绕过，工具层仍会返回 access_denied。
    let mut mcp = McpClient::connect(&endpoint, &token).await;
    let reply = mcp
        .request_as(
            None,
            "tools/call",
            json!({ "name": "list_connections", "arguments": {} }),
        )
        .await;

    assert!(reply.rejected(), "无 Token 的工具调用必须失败：{reply:?}");

    server.stop().await.expect("停止服务失败");
}

/// 安全缺口 T20-GAP-1：`initialize` / `tools/list` 在 rmcp 服务层没有认证中间件，
/// 缺少 `Authorization` 时仍返回 200 与正常结果；只有 `tools/call` 会在
/// `UmtMcpServer::call_tool` 内部鉴权而失败。
///
/// 修复方式：给 `/mcp` 路由加一层 Bearer 校验中间件（或 tower 过滤器），
/// 让未认证请求在进入 rmcp 之前就被拒绝。
///
/// 断言保持「必须拒绝」不变；认证中间件已补上（服务层 `bearer` 中间件）。
#[tokio::test]
async fn unauthenticated_initialize_and_tools_list_must_be_rejected() {
    let env = Harness::new();
    let (_context, _token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("创建 HTTP 客户端失败");

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "umt-t20", "version": "0.1.0" },
        },
    });
    let reply = post(&client, &endpoint, None, None, &initialize).await;
    let initialize_succeeded = reply.succeeded();

    // initialize 若被放行还会带回会话 ID，那就继续探测 tools/list。
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    let reply = post(&client, &endpoint, None, reply.session_id.as_deref(), &list).await;
    let tools_list_succeeded = reply.succeeded();

    assert!(
        !initialize_succeeded && !tools_list_succeeded,
        "未认证请求必须被拒绝，实际 initialize 成功={initialize_succeeded}、tools/list 成功={tools_list_succeeded}：{reply:?}"
    );

    server.stop().await.expect("停止服务失败");
}

/// 非法 Bearer Token 必须被拒绝。
#[tokio::test]
async fn invalid_bearer_token_is_rejected() {
    let env = Harness::new();
    let (_context, token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    let reply = mcp
        .request_as(
            Some("umt_00000000_0123456789abcdef0123456789abcdef"),
            "tools/call",
            json!({ "name": "list_connections", "arguments": {} }),
        )
        .await;
    assert!(reply.rejected(), "非法 Token 必须失败：{reply:?}");

    let reply = mcp
        .request_as(
            Some("Bearer 不是合法 Token"),
            "tools/call",
            json!({ "name": "list_connections", "arguments": {} }),
        )
        .await;
    assert!(reply.result().is_none(), "畸形 Token 必须失败：{reply:?}");

    server.stop().await.expect("停止服务失败");
}

/// 合法 Token 完成握手后，`tools/list` 返回 9 个工具，名称与需求一致。
#[tokio::test]
async fn tools_list_returns_all_nine_tools() {
    let env = Harness::new();
    let (_context, token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    let reply = mcp.request("tools/list", json!({})).await;
    assert_eq!(reply.status, 200, "tools/list 失败：{reply:?}");
    let tools = reply
        .result()
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .expect("tools/list 缺少 tools 数组");

    let mut names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    let mut expected = TOOL_NAMES.to_vec();
    names.sort_unstable();
    expected.sort_unstable();

    assert_eq!(names.len(), TOOL_NAMES.len(), "工具数量不符：{names:?}");
    assert_eq!(names, expected, "工具名称集合与需求不一致");

    server.stop().await.expect("停止服务失败");
}

/// 权限矩阵：新 Token 默认无授权 → 空列表；授权后可见该连接且不泄漏凭据。
#[tokio::test]
async fn list_connections_follows_grants_and_never_leaks_secrets() {
    let env = Harness::new();
    let (context, token) = env.token("claude").await;
    let connection = env.connection("prod-mock").await;
    env.configure(connection.id, json!({ "host": "local" }));
    env.store_secret(
        connection.id,
        br#"{"password":"s3cret-pass","private_key":"-----BEGIN OPENSSH PRIVATE KEY-----"}"#,
    )
    .await;

    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    // 未授权：空列表
    let reply = mcp.tool("list_connections", json!({})).await;
    assert_eq!(reply.status, 200, "list_connections 失败：{reply:?}");
    let result = reply.structured().expect("list_connections 缺少结构化结果");
    assert_eq!(
        result["connections"].as_array().map(Vec::len),
        Some(0),
        "未授权 Token 不应看到任何连接：{result}"
    );
    assert_eq!(result["total"], 0);

    // 授权后：只剩一条，且不含任何凭据字段
    env.grant(&context, connection.id).await;
    let reply = mcp.tool("list_connections", json!({})).await;
    let result = reply.structured().expect("list_connections 缺少结构化结果");
    let connections = result["connections"]
        .as_array()
        .expect("缺少 connections 数组");
    assert_eq!(connections.len(), 1, "授权后应能看到该连接：{result}");
    assert_eq!(connections[0]["connection_id"], connection.id.to_string());

    let text = result.to_string();
    for leaked in ["password", "private_key", "s3cret-pass", "BEGIN OPENSSH"] {
        assert!(
            !text.contains(leaked),
            "list_connections 泄漏了凭据字段 {leaked}：{text}"
        );
    }

    server.stop().await.expect("停止服务失败");
}

/// 越权矩阵：`get_connection_info` 对未授权 connection_id 必须报错；授权后可用。
#[tokio::test]
async fn get_connection_info_rejects_unauthorized_connection_id() {
    let env = Harness::new();
    let (context, token) = env.token("claude").await;
    let granted = env.connection("granted-mock").await;
    let hidden = env.connection("hidden-mock").await;
    env.configure(granted.id, json!({ "host": "local" }));
    env.configure(hidden.id, json!({ "host": "local" }));
    env.grant(&context, granted.id).await;

    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    // 已授权：正常返回，且不包含任何凭据值
    let reply = mcp
        .tool(
            "get_connection_info",
            json!({ "connection_id": granted.id.to_string() }),
        )
        .await;
    let result = reply.structured().expect("已授权连接应返回结构化信息");
    assert_eq!(result["connection_id"], granted.id.to_string());
    assert_eq!(result["has_stored_secrets"], false);
    assert!(!result.to_string().contains("password"));

    // 未授权：换一个 connection_id 越权必须被拒绝
    let reply = mcp
        .tool(
            "get_connection_info",
            json!({ "connection_id": hidden.id.to_string() }),
        )
        .await;
    assert!(
        reply.result().is_none(),
        "未授权 connection_id 必须失败：{reply:?}"
    );
    assert_eq!(reply.error_code(), Some("access_denied"), "{reply:?}");

    // 伪造 ID 同样失败
    let reply = mcp
        .tool(
            "get_connection_info",
            json!({ "connection_id": ConnectionId::new().to_string() }),
        )
        .await;
    assert!(
        reply.result().is_none(),
        "伪造 connection_id 必须失败：{reply:?}"
    );

    server.stop().await.expect("停止服务失败");
}

/// `resize_terminal` 对伪造 session_id 必须返回错误，不能伪造成功。
#[tokio::test]
async fn resize_terminal_with_forged_session_id_fails() {
    let env = Harness::new();
    let (_context, token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    let reply = mcp
        .tool(
            "resize_terminal",
            json!({
                "session_id": SessionId::new().to_string(),
                "rows": 40,
                "cols": 120,
            }),
        )
        .await;
    assert!(
        reply.result().is_none(),
        "伪造 session_id 必须失败：{reply:?}"
    );
    assert_eq!(reply.error_code(), Some("not_found"), "{reply:?}");

    server.stop().await.expect("停止服务失败");
}

/// 撤销 Token 后，下一次调用必须立即失败（实时校验，不依赖缓存）。
#[tokio::test]
async fn revoked_token_is_rejected_on_next_call() {
    let env = Harness::new();
    let (context, token) = env.token("claude").await;
    let (server, endpoint) = env.spawn().await;
    let mut mcp = McpClient::connect(&endpoint, &token).await;

    let reply = mcp.tool("list_connections", json!({})).await;
    assert!(reply.succeeded(), "撤销前应可正常调用：{reply:?}");

    env.access
        .set_token_status(context.token_id, TokenStatus::Revoked)
        .await
        .expect("撤销 Token 失败");

    let reply = mcp.tool("list_connections", json!({})).await;
    assert!(reply.rejected(), "撤销后必须失败：{reply:?}");

    server.stop().await.expect("停止服务失败");
}
