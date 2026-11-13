//! 会话查询与关闭服务。
//!
//! 本地管理界面允许跨 Token 查看会话；关闭时使用「管理员通道」：先从会话记录
//! 读取其真实归属 Token，再以该 Token 身份调用 `SessionManager::close`，因此
//! 仍然复用 SessionManager 的归属校验与审计逻辑，而不是绕过状态机直接改库。

use umt_core::{PageRequest, SessionId, UmtError};
use umt_persistence::{SessionFilter, SessionRepo};

use crate::ipc::dto::{CloseSessionInput, PageDto, SessionDto, SessionFilterDto};
use crate::services::state::AppState;

pub async fn list_active(state: &AppState) -> Result<Vec<SessionDto>, UmtError> {
    let page = state
        .sessions
        .list_active(PageRequest::new(0, PageRequest::MAX_LIMIT))
        .await?;
    Ok(page.items.into_iter().map(SessionDto::from).collect())
}

pub async fn list_history(
    state: &AppState,
    filter: SessionFilterDto,
) -> Result<PageDto<SessionDto>, UmtError> {
    let token_id = filter
        .token_id
        .as_deref()
        .map(|value| crate::services::parse_id(value, "Token"))
        .transpose()?;
    let connection_id = filter
        .connection_id
        .as_deref()
        .map(|value| crate::services::parse_id(value, "连接"))
        .transpose()?;

    let page = state
        .sessions
        .list_history(&SessionFilter {
            token_id,
            connection_id,
            state: None,
            from: None,
            to: None,
            page: filter.page.into(),
        })
        .await?;

    Ok(PageDto {
        items: page.items.into_iter().map(SessionDto::from).collect(),
        total: page.total,
    })
}

pub async fn close(state: &AppState, input: CloseSessionInput) -> Result<bool, UmtError> {
    let session_id = crate::services::parse_id::<SessionId>(&input.session_id, "会话")?;
    let info = state
        .repos
        .sessions
        .get(session_id)
        .await?
        .ok_or_else(|| UmtError::NotFound {
            entity: "会话",
            id: session_id.to_string(),
        })?;
    if !info.state.is_active() {
        return Ok(false);
    }

    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("manual_close");
    // 管理员通道：使用会话自身的 Token 归属调用，避免伪造 Token 上下文。
    state
        .sessions
        .close(session_id, info.token_id, reason)
        .await
}
