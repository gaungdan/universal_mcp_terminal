//! MCP 工具执行层：Bearer 鉴权、连接授权、能力校验与结构化结果。
//!
//! 每个 `tools/call` 都会：
//! 1. 从 HTTP header 解析 `Authorization: Bearer <token>`；
//! 2. 调用 `AccessControl::authenticate` 得到 `TokenContext`；
//! 3. 涉及连接的调用再走 `authorize_connection`；涉及会话的调用由
//!    `SessionManager` 校验会话归属；
//! 4. 按 Provider 声明的 `CapabilitySet` 拒绝实现不了的操作。

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use umt_core::{
    Capability, CapabilitySet, Connection, ConnectionConfig, ConnectionId, PageRequest, SessionId,
    SessionInfo, TokenContext, UmtError,
};
use umt_protocol_api::{ExecRequest, ExecResult, ProtocolProvider, SecretValues};
use umt_security::EncryptedSecret;
use umt_session_manager::OpenSessionRequest;

use crate::deps::McpDeps;
use crate::tools;

/// 默认单次读取输出上限（字节）。
const DEFAULT_READ_BYTES: usize = 64 * 1024;
/// 单次读取输出硬上限，避免一次调用拉爆内存。
const MAX_READ_BYTES: usize = 1024 * 1024;
/// 非交互命令默认超时。
const DEFAULT_EXEC_TIMEOUT_MS: u64 = 30_000;
/// 非交互命令超时上限。
const MAX_EXEC_TIMEOUT_MS: u64 = 600_000;
/// 临时会话的默认终端尺寸（命令执行为非交互，尺寸不影响结果）。
const TEMP_SESSION_ROWS: u16 = 24;
const TEMP_SESSION_COLS: u16 = 80;

/// rmcp 的 `ServerHandler` 实现。
#[derive(Clone)]
pub struct UmtMcpServer {
    deps: Arc<McpDeps>,
}

impl UmtMcpServer {
    pub fn new(deps: Arc<McpDeps>) -> Self {
        Self { deps }
    }

    /// 工具分发的核心入口。抽出为独立方法便于不经 HTTP 直接测试。
    ///
    /// `token` 必须来自 `AccessControl::authenticate`，禁止由工具参数构造。
    pub async fn execute_tool(
        &self,
        token: &TokenContext,
        name: &str,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        match name {
            tools::LIST_CONNECTIONS => self.list_connections(token).await,
            tools::GET_CONNECTION_INFO => self.get_connection_info(token, args).await,
            tools::TEST_CONNECTION => self.test_connection(token, args).await,
            tools::OPEN_SESSION => self.open_session_tool(token, args).await,
            tools::EXECUTE_COMMAND => self.execute_command(token, args).await,
            tools::SEND_INPUT => self.send_input(token, args).await,
            tools::READ_OUTPUT => self.read_output(token, args).await,
            tools::RESIZE_TERMINAL => self.resize_terminal(token, args).await,
            tools::CLOSE_SESSION => self.close_session(token, args).await,
            other => Err(UmtError::NotFound {
                entity: "工具",
                id: other.to_string(),
            }),
        }
    }

    // ------------------------------------------------------------ 连接管理

    async fn list_connections(&self, token: &TokenContext) -> Result<Value, UmtError> {
        let grants = self.deps.access.list_grants(token.token_id).await?;
        let mut items = Vec::new();
        for view in grants {
            // 只返回被授权且启用的连接；未授权连接连名称都不暴露。
            if !view.allowed || !view.connection.enabled {
                continue;
            }
            items.push(self.connection_summary(&view.connection));
        }
        let total = items.len();
        Ok(json!({ "connections": items, "total": total }))
    }

    async fn get_connection_info(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::ConnectionArgs = parse_args(args)?;
        let connection_id = parse_id::<ConnectionId>(&args.connection_id, "连接")?;
        let connection = self
            .deps
            .access
            .authorize_connection(token, connection_id)
            .await?;

        let provider = self.deps.protocols.require(&connection.protocol_id)?;
        let secret_fields: Vec<&str> = provider
            .config_schema()
            .fields
            .iter()
            .filter(|field| field.secret)
            .map(|field| field.key.as_str())
            .collect();
        // 只暴露「是否存在密文」，不返回任何字段值。
        let has_stored_secrets = self.deps.secrets.get(connection_id).await?.is_some();

        let mut payload = self.connection_summary(&connection);
        let object = payload
            .as_object_mut()
            .expect("connection_summary 必须返回对象");
        object.insert("secret_fields".into(), json!(secret_fields));
        object.insert("has_stored_secrets".into(), json!(has_stored_secrets));
        Ok(payload)
    }

    async fn test_connection(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::ConnectionArgs = parse_args(args)?;
        let connection_id = parse_id::<ConnectionId>(&args.connection_id, "连接")?;
        let connection = self
            .deps
            .access
            .authorize_connection(token, connection_id)
            .await?;

        let provider = self.deps.protocols.require(&connection.protocol_id)?;
        let mut config = self.load_config(&connection).await?;
        // Provider::test_connection 只接受 config，敏感字段需要合入值才能完成真实认证。
        let secrets = self.load_secrets(connection_id).await?;
        merge_secrets(&mut config, &secrets, provider.config_schema());

        let result = provider
            .test_connection(&config)
            .await
            .map_err(UmtError::from)?;
        let mut payload = serde_json::to_value(&result)
            .map_err(|err| UmtError::Internal(format!("连接测试结果序列化失败：{err}")))?;
        if let Some(object) = payload.as_object_mut() {
            object.insert("connection_id".into(), json!(connection.id.to_string()));
        }
        Ok(payload)
    }

    // ------------------------------------------------------------- 会话管理

    async fn open_session_tool(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::OpenSessionArgs = parse_args(args)?;
        let connection_id = parse_id::<ConnectionId>(&args.connection_id, "连接")?;
        let connection = self
            .deps
            .access
            .authorize_connection(token, connection_id)
            .await?;
        let provider = self.deps.protocols.require(&connection.protocol_id)?;
        validate_terminal_size(args.rows, args.cols)?;

        let info = self
            .open_session(
                token,
                connection,
                provider,
                args.rows,
                args.cols,
                args.terminal_type,
            )
            .await?;
        Ok(session_value(&info))
    }

    async fn execute_command(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::ExecuteCommandArgs = parse_args(args)?;
        if args.command.trim().is_empty() {
            return Err(UmtError::Validation("command 不能为空".into()));
        }
        let request = ExecRequest {
            command: args.command,
            working_dir: args.working_directory,
            timeout_ms: args
                .timeout_ms
                .unwrap_or(DEFAULT_EXEC_TIMEOUT_MS)
                .clamp(1, MAX_EXEC_TIMEOUT_MS),
            max_output_bytes: 256 * 1024,
        };

        match (args.connection_id, args.session_id) {
            (Some(_), Some(_)) => Err(UmtError::Validation(
                "connection_id 与 session_id 只能二选一".into(),
            )),
            (None, None) => Err(UmtError::Validation(
                "必须提供 connection_id 或 session_id".into(),
            )),
            (Some(raw), None) => {
                let connection_id = parse_id::<ConnectionId>(&raw, "连接")?;
                let connection = self
                    .deps
                    .access
                    .authorize_connection(token, connection_id)
                    .await?;
                let provider = self.deps.protocols.require(&connection.protocol_id)?;
                ensure_capability(&provider.descriptor().capabilities, Capability::Exec)?;

                let info = self
                    .open_session(
                        token,
                        connection,
                        provider,
                        TEMP_SESSION_ROWS,
                        TEMP_SESSION_COLS,
                        None,
                    )
                    .await?;
                let outcome = self
                    .deps
                    .sessions
                    .exec(info.id, token.token_id, request)
                    .await;
                // 临时会话无论成功失败都要关闭，避免泄漏连接。
                let _ = self
                    .deps
                    .sessions
                    .close(info.id, token.token_id, "exec_finished")
                    .await;
                let result = outcome?;
                Ok(exec_value(&info, &result))
            }
            (None, Some(raw)) => {
                let session_id = parse_id::<SessionId>(&raw, "会话")?;
                let info = self.session_info(token, session_id).await?;
                let provider = self.deps.protocols.require(&info.protocol_id)?;
                ensure_capability(&provider.descriptor().capabilities, Capability::Exec)?;

                let result = self
                    .deps
                    .sessions
                    .exec(session_id, token.token_id, request)
                    .await?;
                Ok(exec_value(&info, &result))
            }
        }
    }

    async fn send_input(&self, token: &TokenContext, args: &JsonObject) -> Result<Value, UmtError> {
        let args: tools::SendInputArgs = parse_args(args)?;
        let session_id = parse_id::<SessionId>(&args.session_id, "会话")?;
        // SessionManager 会校验会话归属，跨 Token 操作直接拒绝。
        self.deps
            .sessions
            .write(session_id, token.token_id, args.input.as_bytes())
            .await?;
        Ok(json!({
            "session_id": session_id.to_string(),
            "bytes_sent": args.input.len(),
        }))
    }

    async fn read_output(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::ReadOutputArgs = parse_args(args)?;
        let session_id = parse_id::<SessionId>(&args.session_id, "会话")?;
        let max_bytes = args
            .max_bytes
            .unwrap_or(DEFAULT_READ_BYTES)
            .clamp(1, MAX_READ_BYTES);
        let chunk = self
            .deps
            .sessions
            .read(session_id, token.token_id, args.cursor, max_bytes)
            .await?;
        Ok(json!({
            "session_id": session_id.to_string(),
            "cursor": chunk.cursor,
            "text": chunk.text,
            "eof": chunk.eof,
            "truncated": chunk.truncated,
        }))
    }

    async fn resize_terminal(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::ResizeTerminalArgs = parse_args(args)?;
        validate_terminal_size(args.rows, args.cols)?;
        let session_id = parse_id::<SessionId>(&args.session_id, "会话")?;
        let info = self.session_info(token, session_id).await?;
        let provider = self.deps.protocols.require(&info.protocol_id)?;
        ensure_capability(
            &provider.descriptor().capabilities,
            Capability::TerminalResize,
        )?;

        self.deps
            .sessions
            .resize(session_id, token.token_id, args.rows, args.cols)
            .await?;
        Ok(json!({
            "session_id": session_id.to_string(),
            "rows": args.rows,
            "cols": args.cols,
            "resized": true,
        }))
    }

    async fn close_session(
        &self,
        token: &TokenContext,
        args: &JsonObject,
    ) -> Result<Value, UmtError> {
        let args: tools::CloseSessionArgs = parse_args(args)?;
        let session_id = parse_id::<SessionId>(&args.session_id, "会话")?;
        let closed = self
            .deps
            .sessions
            .close(session_id, token.token_id, "mcp_close")
            .await?;
        Ok(json!({
            "session_id": session_id.to_string(),
            "closed": closed,
        }))
    }

    // --------------------------------------------------------------- 内部实现

    async fn open_session(
        &self,
        token: &TokenContext,
        connection: Connection,
        provider: Arc<dyn ProtocolProvider>,
        rows: u16,
        cols: u16,
        terminal_type: Option<String>,
    ) -> Result<SessionInfo, UmtError> {
        let config = self.load_config(&connection).await?;
        let secrets = self.load_secrets(connection.id).await?;
        let expected_host_key_fingerprint = self.expected_host_key(&config).await?;

        self.deps
            .sessions
            .open(
                provider,
                OpenSessionRequest {
                    context: token.clone(),
                    connection,
                    config,
                    secrets,
                    expected_host_key_fingerprint,
                    rows,
                    cols,
                    terminal_type,
                    working_dir: None,
                },
            )
            .await
    }

    async fn load_config(&self, connection: &Connection) -> Result<ConnectionConfig, UmtError> {
        let config = self
            .deps
            .configs
            .get_config(connection.id)
            .await?
            .ok_or_else(|| UmtError::NotFound {
                entity: "连接配置",
                id: connection.id.to_string(),
            })?;
        if config.protocol_id != connection.protocol_id {
            return Err(UmtError::Validation(format!(
                "连接 {} 的配置协议 {} 与连接协议 {} 不一致",
                connection.name, config.protocol_id, connection.protocol_id
            )));
        }
        Ok(config)
    }

    /// 解密凭据。密文格式为 JSON 对象（字段名 → 明文值），只在内存中使用。
    async fn load_secrets(&self, connection_id: ConnectionId) -> Result<SecretValues, UmtError> {
        let Some(blob) = self.deps.secrets.get(connection_id).await? else {
            return Ok(SecretValues::default());
        };
        if blob.key_version != self.deps.cipher.key_version() {
            return Err(UmtError::Storage(format!(
                "凭据密钥版本不匹配：密文 {}，当前 {}",
                blob.key_version,
                self.deps.cipher.key_version()
            )));
        }
        let nonce: [u8; 12] = blob
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| UmtError::Storage("凭据随机数长度非法".into()))?;
        let plaintext = self
            .deps
            .cipher
            .decrypt(&EncryptedSecret {
                ciphertext: blob.ciphertext,
                nonce,
                key_version: blob.key_version,
            })
            .map_err(UmtError::from)?;
        let values: BTreeMap<String, String> = serde_json::from_slice(plaintext.as_slice())
            .map_err(|_| UmtError::Storage("凭据内容不是合法的字段映射".into()))?;
        Ok(SecretValues::new(values))
    }

    /// 已信任的 Host Key 指纹。没有记录时返回 `None`，由 Provider 拒绝未知指纹。
    async fn expected_host_key(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Option<String>, UmtError> {
        let (Some(host), Some(port)) = (config.get_str("host"), config.get_u16("port")) else {
            return Ok(None);
        };
        let endpoint = format!("{host}:{port}");
        Ok(self
            .deps
            .host_keys
            .get(&endpoint)
            .await?
            .map(|record| record.fingerprint))
    }

    /// 查询会话归属与协议。跨 Token 的会话一律按不存在处理，避免泄漏。
    async fn session_info(
        &self,
        token: &TokenContext,
        session_id: SessionId,
    ) -> Result<SessionInfo, UmtError> {
        let page = self
            .deps
            .sessions
            .list_active(PageRequest::new(0, PageRequest::MAX_LIMIT))
            .await?;
        page.items
            .into_iter()
            .find(|info| info.id == session_id && info.token_id == token.token_id)
            .ok_or_else(|| UmtError::NotFound {
                entity: "会话",
                id: session_id.to_string(),
            })
    }

    fn connection_summary(&self, connection: &Connection) -> Value {
        json!({
            "connection_id": connection.id.to_string(),
            "name": connection.name,
            "protocol": connection.protocol_id,
            "description": connection.description,
            "capabilities": self.capabilities_of(&connection.protocol_id),
            "enabled": connection.enabled,
        })
    }

    fn capabilities_of(&self, protocol_id: &str) -> Vec<&'static str> {
        self.deps
            .protocols
            .get(protocol_id)
            .map(|provider| provider.descriptor().capabilities.as_strings())
            .unwrap_or_default()
    }
}

impl ServerHandler for UmtMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "universal-mcp-terminal",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "请通过 Authorization: Bearer <token> 访问；每个工具调用都会校验 Token 与连接授权。",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools::tool_definitions(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // 鉴权信息只允许来自 HTTP header，绝不相信工具参数。
        let bearer = bearer_from_context(&context).map_err(to_mcp_error)?;
        let client_info = user_agent(&context);
        let token = self
            .deps
            .access
            .authenticate(bearer, client_info)
            .await
            .map_err(to_mcp_error)?;

        let name = request.name.to_string();
        if !tools::TOOL_NAMES.contains(&name.as_str()) {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("未知工具：{name}"),
                None,
            ));
        }

        let args = request.arguments.unwrap_or_default();
        let payload = self
            .execute_tool(&token, &name, &args)
            .await
            .map_err(to_mcp_error)?;
        Ok(CallToolResult::structured(payload).into())
    }
}

// ---------------------------------------------------------------- 鉴权辅助

/// 从请求头解析 Bearer Token。缺失、非 Bearer、空值一律拒绝。
pub fn parse_bearer(value: Option<&str>) -> Result<&str, UmtError> {
    let value = value.ok_or_else(|| UmtError::AccessDenied("缺少 Authorization 头".into()))?;
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(UmtError::AccessDenied(
            "Authorization 必须使用 Bearer 方案".into(),
        ));
    }
    let token = parts.next().unwrap_or_default().trim();
    if token.is_empty() {
        return Err(UmtError::AccessDenied("Bearer Token 不能为空".into()));
    }
    Ok(token)
}

fn bearer_from_context(context: &RequestContext<RoleServer>) -> Result<&str, UmtError> {
    let header = context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.headers.get(http::header::AUTHORIZATION))
        .and_then(|value| value.to_str().ok());
    parse_bearer(header)
}

fn user_agent(context: &RequestContext<RoleServer>) -> Option<String> {
    context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.headers.get(http::header::USER_AGENT))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn to_mcp_error(error: UmtError) -> ErrorData {
    // 只记录错误码与脱敏后的错误文本，绝不记录 Token/密码/私钥。
    tracing::warn!(code = error.code(), message = %error, "MCP 工具调用失败");
    let code = match &error {
        UmtError::NotFound { .. } => ErrorCode::RESOURCE_NOT_FOUND,
        UmtError::Validation(_) => ErrorCode::INVALID_PARAMS,
        UmtError::Internal(_) | UmtError::Storage(_) => ErrorCode::INTERNAL_ERROR,
        UmtError::AccessDenied(_) => ErrorCode(-32001),
        UmtError::UnsupportedCapability(_) => ErrorCode(-32003),
        UmtError::Conflict(_) | UmtError::Timeout(_) | UmtError::External(_) => ErrorCode(-32000),
    };
    let message = error.to_string();
    ErrorData::new(code, message, Some(json!({ "code": error.code() })))
}

// ---------------------------------------------------------------- 通用辅助

fn parse_args<T: DeserializeOwned>(args: &JsonObject) -> Result<T, UmtError> {
    serde_json::from_value(Value::Object(args.clone()))
        .map_err(|err| UmtError::Validation(format!("工具参数无效：{err}")))
}

fn parse_id<T: FromStr>(raw: &str, entity: &str) -> Result<T, UmtError>
where
    T::Err: std::fmt::Display,
{
    raw.trim()
        .parse::<T>()
        .map_err(|err| UmtError::Validation(format!("{entity} ID 无效：{err}")))
}

fn validate_terminal_size(rows: u16, cols: u16) -> Result<(), UmtError> {
    if rows == 0 || cols == 0 {
        return Err(UmtError::Validation("rows 与 cols 必须大于 0".into()));
    }
    Ok(())
}

fn ensure_capability(capabilities: &CapabilitySet, capability: Capability) -> Result<(), UmtError> {
    if capabilities.contains(capability) {
        Ok(())
    } else {
        Err(UmtError::UnsupportedCapability(capability))
    }
}

fn merge_secrets(
    config: &mut ConnectionConfig,
    secrets: &SecretValues,
    schema: &umt_protocol_api::ConfigSchema,
) {
    let Some(values) = config.values.as_object_mut() else {
        return;
    };
    // 只有 Schema 声明为敏感、且确实存在的字段才合入，避免把任意字段塞进配置。
    for field in schema.fields.iter().filter(|field| field.secret) {
        if let Some(secret) = secrets.get(&field.key) {
            values.insert(field.key.clone(), Value::String(secret.to_string()));
        }
    }
}

fn session_value(info: &SessionInfo) -> Value {
    json!({
        "session_id": info.id.to_string(),
        "connection_id": info.connection_id.to_string(),
        "protocol": info.protocol_id,
        "state": info.state,
        "rows": info.rows,
        "cols": info.cols,
        "started_at": info.started_at.0,
    })
}

fn exec_value(info: &SessionInfo, result: &ExecResult) -> Value {
    json!({
        "session_id": info.id.to_string(),
        "connection_id": info.connection_id.to_string(),
        "state": result.state,
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "duration_ms": result.duration_ms,
        "truncated": result.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use umt_core::UmtError;
    use umt_persistence::{SecretBlob, SecretRepo, SessionRepo};

    use crate::testing::TestEnv;

    fn args(value: Value) -> JsonObject {
        match value {
            Value::Object(map) => map,
            _ => panic!("工具参数必须是 JSON 对象"),
        }
    }

    #[test]
    fn bearer_parsing_rejects_missing_and_malformed_headers() {
        assert!(parse_bearer(None).is_err());
        assert!(parse_bearer(Some("")).is_err());
        assert!(parse_bearer(Some("Basic abc")).is_err());
        assert!(parse_bearer(Some("Bearer")).is_err());
        assert!(parse_bearer(Some("Bearer ")).is_err());
        assert!(parse_bearer(Some("Bearer   ")).is_err());
        assert_eq!(
            parse_bearer(Some("Bearer umt_abc")).expect("应通过"),
            "umt_abc"
        );
        assert_eq!(
            parse_bearer(Some("bearer umt_abc")).expect("scheme 应大小写不敏感"),
            "umt_abc"
        );
    }

    #[tokio::test]
    async fn unauthorized_connection_is_rejected() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let connection = env.connection("prod", "mock").await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));

        let error = server
            .execute_tool(
                &token,
                tools::GET_CONNECTION_INFO,
                &args(json!({ "connection_id": connection.id.to_string() })),
            )
            .await
            .expect_err("未授权连接必须被拒绝");
        assert_eq!(error.code(), "access_denied");
    }

    #[tokio::test]
    async fn list_connections_only_returns_granted_and_enabled() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let granted = env.connection("prod", "mock").await;
        let _hidden = env.connection("staging", "mock").await;
        env.grant(&token, granted.id).await;

        let payload = server
            .execute_tool(&token, tools::LIST_CONNECTIONS, &JsonObject::new())
            .await
            .expect("列连接失败");

        let connections = payload["connections"].as_array().expect("缺少 connections");
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0]["connection_id"], granted.id.to_string());
        assert_eq!(connections[0]["protocol"], "mock");
        // 不得出现任何凭据字段
        let text = payload.to_string();
        assert!(!text.contains("password"));
        assert!(!text.contains("private_key"));
    }

    #[test]
    fn unsupported_capability_is_reported() {
        let shell_only = CapabilitySet::of([Capability::Shell, Capability::InteractiveTerminal]);
        let error = ensure_capability(&shell_only, Capability::Exec).expect_err("应拒绝");
        assert_eq!(error, UmtError::UnsupportedCapability(Capability::Exec));
        assert!(ensure_capability(&shell_only, Capability::Shell).is_ok());
    }

    #[tokio::test]
    async fn full_session_flow_works_over_mock_provider() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let connection = env.connection("mock-conn", "mock").await;
        env.grant(&token, connection.id).await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));
        env.store_secret(connection.id, br#"{"password":"s3cret"}"#)
            .await;

        let opened = server
            .execute_tool(
                &token,
                tools::OPEN_SESSION,
                &args(json!({
                    "connection_id": connection.id.to_string(),
                    "rows": 24,
                    "cols": 80,
                })),
            )
            .await
            .expect("打开会话失败");
        let session_id = opened["session_id"]
            .as_str()
            .expect("缺少 session_id")
            .to_string();
        assert_eq!(opened["protocol"], "mock");

        server
            .execute_tool(
                &token,
                tools::SEND_INPUT,
                &args(json!({ "session_id": session_id, "input": "hello" })),
            )
            .await
            .expect("发送输入失败");

        let output = server
            .execute_tool(
                &token,
                tools::READ_OUTPUT,
                &args(json!({ "session_id": session_id, "cursor": 0 })),
            )
            .await
            .expect("读取输出失败");
        assert_eq!(output["text"], "hello");
        assert_eq!(output["cursor"], 5);
        assert_eq!(output["eof"], false);

        let executed = server
            .execute_tool(
                &token,
                tools::EXECUTE_COMMAND,
                &args(json!({ "session_id": session_id, "command": "uname" })),
            )
            .await
            .expect("执行命令失败");
        assert_eq!(executed["stdout"], "mock:uname");
        assert_eq!(executed["state"], "succeeded");

        server
            .execute_tool(
                &token,
                tools::RESIZE_TERMINAL,
                &args(json!({ "session_id": session_id, "rows": 40, "cols": 120 })),
            )
            .await
            .expect("调整尺寸失败");

        let closed = server
            .execute_tool(
                &token,
                tools::CLOSE_SESSION,
                &args(json!({ "session_id": session_id })),
            )
            .await
            .expect("关闭会话失败");
        assert_eq!(closed["closed"], true);

        // 关闭后不能再操作
        assert!(
            server
                .execute_tool(
                    &token,
                    tools::SEND_INPUT,
                    &args(json!({ "session_id": session_id, "input": "x" })),
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn execute_command_with_connection_opens_and_closes_temp_session() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let connection = env.connection("mock-conn", "mock").await;
        env.grant(&token, connection.id).await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));

        let payload = server
            .execute_tool(
                &token,
                tools::EXECUTE_COMMAND,
                &args(json!({
                    "connection_id": connection.id.to_string(),
                    "command": "id",
                    "timeout_ms": 5_000,
                })),
            )
            .await
            .expect("执行命令失败");
        assert_eq!(payload["stdout"], "mock:id");
        assert_eq!(payload["exit_code"], 0);

        // 临时会话必须已关闭
        let active = env
            .repos
            .sessions
            .list_active(PageRequest::new(0, 10))
            .await
            .expect("查询活动会话失败");
        assert!(active.items.is_empty(), "临时会话应已关闭");
    }

    #[tokio::test]
    async fn another_token_cannot_touch_the_session() {
        let env = TestEnv::new();
        let server = env.server();
        let owner = env.token("owner").await;
        let intruder = env.token("intruder").await;
        let connection = env.connection("mock-conn", "mock").await;
        env.grant(&owner, connection.id).await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));

        let opened = server
            .execute_tool(
                &owner,
                tools::OPEN_SESSION,
                &args(json!({
                    "connection_id": connection.id.to_string(),
                    "rows": 24,
                    "cols": 80,
                })),
            )
            .await
            .expect("打开会话失败");
        let session_id = opened["session_id"]
            .as_str()
            .expect("缺少 session_id")
            .to_string();

        let error = server
            .execute_tool(
                &intruder,
                tools::SEND_INPUT,
                &args(json!({ "session_id": session_id, "input": "rm -rf /" })),
            )
            .await
            .expect_err("跨 Token 操作必须失败");
        assert_eq!(error.code(), "access_denied");

        let error = server
            .execute_tool(
                &intruder,
                tools::RESIZE_TERMINAL,
                &args(json!({ "session_id": session_id, "rows": 10, "cols": 10 })),
            )
            .await
            .expect_err("跨 Token 调整尺寸必须失败");
        assert_eq!(error.code(), "not_found");
    }

    #[tokio::test]
    async fn key_version_mismatch_is_reported() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let connection = env.connection("mock-conn", "mock").await;
        env.grant(&token, connection.id).await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));
        env.repos
            .secrets
            .put(
                connection.id,
                SecretBlob {
                    ciphertext: vec![1, 2, 3],
                    nonce: vec![0u8; 12],
                    key_version: 99,
                },
            )
            .await
            .expect("写入密文失败");

        let error = server
            .execute_tool(
                &token,
                tools::OPEN_SESSION,
                &args(json!({
                    "connection_id": connection.id.to_string(),
                    "rows": 24,
                    "cols": 80,
                })),
            )
            .await
            .expect_err("密钥版本不匹配必须失败");
        assert!(
            error.to_string().contains("密钥版本不匹配"),
            "实际错误：{error}"
        );
    }

    #[tokio::test]
    async fn get_connection_info_never_leaks_secret_values() {
        let env = TestEnv::new();
        let server = env.server();
        let token = env.token("claude").await;
        let connection = env.connection("mock-conn", "mock").await;
        env.grant(&token, connection.id).await;
        env.configure(connection.id, "mock", json!({ "host": "local" }));
        env.store_secret(connection.id, br#"{"password":"s3cret"}"#)
            .await;

        let payload = server
            .execute_tool(
                &token,
                tools::GET_CONNECTION_INFO,
                &args(json!({ "connection_id": connection.id.to_string() })),
            )
            .await
            .expect("获取连接信息失败");
        assert_eq!(payload["has_stored_secrets"], true);
        assert!(!payload.to_string().contains("s3cret"));
    }
}
