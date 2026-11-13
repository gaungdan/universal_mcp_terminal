//! Tauri 命令契约（T5 冻结）。
//!
//! 参数名与 DTO 字段保持不变；实现全部下沉到 `crate::services`，本层只做参数透传
//! 与 `UmtError -> ApiError` 转换。前端调用方式见 `apps/desktop/src/api/client.ts`。

use tauri::State;

use crate::ipc::dto::{
    ApiError, AuditDto, AuditQueryDto, CloseSessionInput, ConnectionDetailDto, ConnectionDto,
    ConnectionFilterDto, ConnectionTestDto, CreateConnectionInput, CreateTokenInput,
    CreatedTokenDto, DraftConnectionTestInput, ExportResultDto, GrantDto, McpConfigInput,
    McpStatusDto, OverviewDto, PageDto, PageRequestDto, ProtocolInfoDto, SessionDto,
    SessionFilterDto, SetGrantInput, SetTokenStatusInput, SettingsDto, TokenDto,
    UpdateConnectionInput, UpdateSettingsInput, UpdateTokenInput,
};
use crate::services;
use crate::services::state::AppState;

// ---------------- 概览与协议 ----------------

#[tauri::command]
pub async fn get_overview(state: State<'_, AppState>) -> Result<OverviewDto, ApiError> {
    Ok(services::settings::overview(state.inner()).await?)
}

#[tauri::command]
pub async fn list_protocols(state: State<'_, AppState>) -> Result<Vec<ProtocolInfoDto>, ApiError> {
    Ok(services::protocols::list(state.inner()))
}

// ---------------- 连接管理 ----------------

#[tauri::command]
pub async fn list_connections(
    state: State<'_, AppState>,
    filter: ConnectionFilterDto,
) -> Result<PageDto<ConnectionDto>, ApiError> {
    Ok(services::connections::list(state.inner(), filter).await?)
}

#[tauri::command]
pub async fn get_connection(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionDetailDto, ApiError> {
    Ok(services::connections::get(state.inner(), &id).await?)
}

#[tauri::command]
pub async fn create_connection(
    state: State<'_, AppState>,
    input: CreateConnectionInput,
) -> Result<ConnectionDto, ApiError> {
    Ok(services::connections::create(state.inner(), input).await?)
}

#[tauri::command]
pub async fn update_connection(
    state: State<'_, AppState>,
    input: UpdateConnectionInput,
) -> Result<ConnectionDto, ApiError> {
    Ok(services::connections::update(state.inner(), input).await?)
}

#[tauri::command]
pub async fn delete_connection(state: State<'_, AppState>, id: String) -> Result<bool, ApiError> {
    Ok(services::connections::delete(state.inner(), &id).await?)
}

#[tauri::command]
pub async fn set_connection_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<ConnectionDto, ApiError> {
    Ok(services::connections::set_enabled(state.inner(), &id, enabled).await?)
}

#[tauri::command]
pub async fn duplicate_connection(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionDto, ApiError> {
    Ok(services::connections::duplicate(state.inner(), &id).await?)
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionTestDto, ApiError> {
    Ok(services::connections::test(state.inner(), &id).await?)
}

#[tauri::command]
pub async fn test_connection_draft(
    state: State<'_, AppState>,
    input: DraftConnectionTestInput,
) -> Result<ConnectionTestDto, ApiError> {
    Ok(services::connections::test_draft(state.inner(), input).await?)
}

// ---------------- 访问控制 ----------------

#[tauri::command]
pub async fn list_tokens(
    state: State<'_, AppState>,
    page: PageRequestDto,
) -> Result<PageDto<TokenDto>, ApiError> {
    Ok(services::tokens::list(state.inner(), page).await?)
}

#[tauri::command]
pub async fn create_token(
    state: State<'_, AppState>,
    input: CreateTokenInput,
) -> Result<CreatedTokenDto, ApiError> {
    Ok(services::tokens::create(state.inner(), input).await?)
}

#[tauri::command]
pub async fn update_token(
    state: State<'_, AppState>,
    input: UpdateTokenInput,
) -> Result<TokenDto, ApiError> {
    Ok(services::tokens::update(state.inner(), input).await?)
}

#[tauri::command]
pub async fn set_token_status(
    state: State<'_, AppState>,
    input: SetTokenStatusInput,
) -> Result<TokenDto, ApiError> {
    Ok(services::tokens::set_status(state.inner(), input).await?)
}

#[tauri::command]
pub async fn delete_token(state: State<'_, AppState>, id: String) -> Result<bool, ApiError> {
    Ok(services::tokens::delete(state.inner(), &id).await?)
}

#[tauri::command]
pub async fn list_token_grants(
    state: State<'_, AppState>,
    token_id: String,
) -> Result<Vec<GrantDto>, ApiError> {
    Ok(services::tokens::list_grants(state.inner(), &token_id).await?)
}

#[tauri::command]
pub async fn set_token_grant(
    state: State<'_, AppState>,
    input: SetGrantInput,
) -> Result<GrantDto, ApiError> {
    Ok(services::tokens::set_grant(state.inner(), input).await?)
}

// ---------------- 会话 ----------------

#[tauri::command]
pub async fn list_active_sessions(state: State<'_, AppState>) -> Result<Vec<SessionDto>, ApiError> {
    Ok(services::sessions::list_active(state.inner()).await?)
}

#[tauri::command]
pub async fn list_session_history(
    state: State<'_, AppState>,
    filter: SessionFilterDto,
) -> Result<PageDto<SessionDto>, ApiError> {
    Ok(services::sessions::list_history(state.inner(), filter).await?)
}

#[tauri::command]
pub async fn close_session(
    state: State<'_, AppState>,
    input: CloseSessionInput,
) -> Result<bool, ApiError> {
    Ok(services::sessions::close(state.inner(), input).await?)
}

// ---------------- 审计 ----------------

#[tauri::command]
pub async fn query_audit(
    state: State<'_, AppState>,
    query: AuditQueryDto,
) -> Result<PageDto<AuditDto>, ApiError> {
    Ok(services::audit::query(state.inner(), query).await?)
}

#[tauri::command]
pub async fn get_audit(state: State<'_, AppState>, id: i64) -> Result<AuditDto, ApiError> {
    Ok(services::audit::get(state.inner(), id).await?)
}

#[tauri::command]
pub async fn export_audit(
    state: State<'_, AppState>,
    query: AuditQueryDto,
) -> Result<ExportResultDto, ApiError> {
    Ok(services::audit::export(state.inner(), query).await?)
}

// ---------------- MCP 服务 ----------------

#[tauri::command]
pub async fn get_mcp_status(state: State<'_, AppState>) -> Result<McpStatusDto, ApiError> {
    Ok(services::mcp::status(state.inner()).await)
}

#[tauri::command]
pub async fn start_mcp(
    state: State<'_, AppState>,
    config: Option<McpConfigInput>,
) -> Result<McpStatusDto, ApiError> {
    Ok(services::mcp::start(state.inner(), config).await?)
}

#[tauri::command]
pub async fn stop_mcp(state: State<'_, AppState>) -> Result<McpStatusDto, ApiError> {
    Ok(services::mcp::stop(state.inner()).await?)
}

#[tauri::command]
pub async fn update_mcp_settings(
    state: State<'_, AppState>,
    input: McpConfigInput,
) -> Result<McpStatusDto, ApiError> {
    Ok(services::mcp::update_settings(state.inner(), input).await?)
}

// ---------------- 系统设置 ----------------

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, ApiError> {
    Ok(services::settings::get(state.inner()).await?)
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    input: UpdateSettingsInput,
) -> Result<SettingsDto, ApiError> {
    Ok(services::settings::update(state.inner(), input).await?)
}
