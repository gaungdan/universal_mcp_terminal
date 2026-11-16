use serde::{Deserialize, Serialize};
use umt_core::{
    AppEvent, AuditKind, AuditOutcome, Page, PageRequest, SessionInfo, Token, TokenPermission,
    TokenStatus, UmtError, UnixMillis,
};
use umt_protocol_api::{ConfigSchema, ConnectionTestResult, ExecResult};

/// IPC 统一错误结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn unimplemented(command: &str) -> Self {
        Self::new("not_implemented", format!("命令 {command} 尚未实现"))
    }
}

impl From<UmtError> for ApiError {
    fn from(value: UmtError) -> Self {
        Self {
            code: value.code().to_string(),
            message: value.to_string(),
        }
    }
}

/// 通用分页响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDto<T> {
    pub items: Vec<T>,
    pub total: u64,
}

impl<T> From<Page<T>> for PageDto<T> {
    fn from(value: Page<T>) -> Self {
        Self {
            items: value.items,
            total: value.total,
        }
    }
}

/// 分页请求。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRequestDto {
    pub offset: u32,
    pub limit: u32,
}

impl From<PageRequestDto> for PageRequest {
    fn from(value: PageRequestDto) -> Self {
        PageRequest::new(value.offset, value.limit).normalized()
    }
}

/// 协议列表项。前端据此渲染协议选择与动态表单。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInfoDto {
    pub protocol_id: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub config_schema: ConfigSchema,
}

/// 连接列表项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDto {
    pub id: String,
    pub name: String,
    pub protocol_id: String,
    pub protocol_display_name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub has_secret: bool,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
    pub revision: u32,
}

/// 连接详情：额外返回配置值（敏感字段永远不回显）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDetailDto {
    pub connection: ConnectionDto,
    pub config: serde_json::Value,
    /// 已配置的敏感字段名，前端据此显示"已设置/未设置"。
    pub secret_fields_set: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretInput {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionInput {
    pub name: String,
    pub protocol_id: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub secrets: Vec<SecretInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionInput {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub config: serde_json::Value,
    /// `None` 表示保留已有敏感字段；`Some` 表示整体替换。
    pub secrets: Option<Vec<SecretInput>>,
    pub expected_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionFilterDto {
    pub protocol_id: Option<String>,
    pub enabled: Option<bool>,
    pub search: Option<String>,
    #[serde(flatten)]
    pub page: PageRequestDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftConnectionTestInput {
    pub protocol_id: String,
    pub config: serde_json::Value,
    pub secrets: Vec<SecretInput>,
}

/// 连接测试结果直接复用协议层结构。
pub type ConnectionTestDto = ConnectionTestResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub prefix: String,
    pub status: TokenStatus,
    pub permission: TokenPermission,
    pub allow_interactive: bool,
    pub expires_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
    pub last_used_at: Option<UnixMillis>,
}

impl From<Token> for TokenDto {
    fn from(value: Token) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            description: value.description,
            prefix: value.prefix,
            status: value.status,
            permission: value.permission,
            allow_interactive: value.allow_interactive,
            expires_at: value.expires_at,
            created_at: value.created_at,
            last_used_at: value.last_used_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenInput {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub permission: TokenPermission,
    #[serde(default)]
    pub allow_interactive: bool,
    pub expires_at: Option<UnixMillis>,
}

/// 创建 Token 的响应：`plaintext` 只出现这一次。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTokenDto {
    pub token: TokenDto,
    pub plaintext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTokenInput {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub permission: TokenPermission,
    #[serde(default)]
    pub allow_interactive: bool,
    pub expires_at: Option<UnixMillis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTokenStatusInput {
    pub id: String,
    pub status: TokenStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantDto {
    pub connection: ConnectionDto,
    pub allowed: bool,
    pub granted_at: Option<UnixMillis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGrantInput {
    pub token_id: String,
    pub connection_id: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub token_id: String,
    pub connection_id: String,
    pub protocol_id: String,
    pub client_info: Option<String>,
    pub state: String,
    pub started_at: UnixMillis,
    pub ended_at: Option<UnixMillis>,
    pub close_reason: Option<String>,
    pub rows: u16,
    pub cols: u16,
}

impl From<SessionInfo> for SessionDto {
    fn from(value: SessionInfo) -> Self {
        Self {
            id: value.id.to_string(),
            token_id: value.token_id.to_string(),
            connection_id: value.connection_id.to_string(),
            protocol_id: value.protocol_id,
            client_info: value.client_info,
            state: format!("{:?}", value.state).to_lowercase(),
            started_at: value.started_at,
            ended_at: value.ended_at,
            close_reason: value.close_reason,
            rows: value.rows,
            cols: value.cols,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFilterDto {
    pub token_id: Option<String>,
    pub connection_id: Option<String>,
    #[serde(flatten)]
    pub page: PageRequestDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionInput {
    pub session_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditDto {
    pub id: i64,
    pub session_id: Option<String>,
    pub token_id: Option<String>,
    pub connection_id: Option<String>,
    pub kind: AuditKind,
    pub outcome: AuditOutcome,
    pub tool_name: Option<String>,
    pub permission: Option<TokenPermission>,
    pub command: Option<String>,
    pub working_dir: Option<String>,
    pub started_at: UnixMillis,
    pub duration_ms: Option<u64>,
    pub state: String,
    pub exit_code: Option<i32>,
    pub stdout_excerpt: Option<String>,
    pub stderr_excerpt: Option<String>,
    pub error: Option<String>,
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQueryDto {
    pub token_id: Option<String>,
    pub connection_id: Option<String>,
    pub session_id: Option<String>,
    pub kind: Option<AuditKind>,
    pub outcome: Option<AuditOutcome>,
    pub from: Option<UnixMillis>,
    pub to: Option<UnixMillis>,
    #[serde(flatten)]
    pub page: PageRequestDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    pub path: String,
    pub rows: u64,
}

/// 命令执行结果（供 MCP 调试面板与后续交互式终端使用）。
pub type ExecResultDto = ExecResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigInput {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettingsDto {
    pub default_execution_mode: String,
    pub session_timeout_secs: u64,
    pub require_confirmation_for_high_risk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub mcp: McpStatusDto,
    pub security: SecuritySettingsDto,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub security: SecuritySettingsDto,
    pub log_level: String,
}

/// 首页概览。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewDto {
    pub connection_count: u64,
    pub enabled_connection_count: u64,
    pub token_count: u64,
    pub active_session_count: u64,
    pub mcp: McpStatusDto,
    pub recent_audits: Vec<AuditDto>,
}

/// 已信任的 SSH Host Key。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyDto {
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    pub algorithm: Option<String>,
    pub fingerprint: String,
    pub trusted_at: UnixMillis,
    pub trusted_by: Option<String>,
}

/// 信任某个连接的 Host Key。`endpoint` 由后端根据连接配置推导，前端不参与拼接。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustHostKeyInput {
    pub connection_id: String,
    pub fingerprint: String,
    pub algorithm: Option<String>,
}

/// 应用事件负载，事件名固定为 `umt://event`。
pub type AppEventDto = AppEvent;
