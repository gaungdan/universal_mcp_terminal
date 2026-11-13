//! 访问 Token 与授权矩阵服务。

use std::collections::BTreeMap;

use umt_core::{AuditKind, AuditOutcome, ConnectionId, TokenId, TokenStatus, UmtError, UnixMillis};
use umt_persistence::GrantView;

use crate::ipc::dto::{
    CreateTokenInput, CreatedTokenDto, GrantDto, PageDto, PageRequestDto, SetGrantInput,
    SetTokenStatusInput, TokenDto, UpdateTokenInput,
};
use crate::services::audit::record_event;
use crate::services::connections::to_dto;
use crate::services::state::AppState;

pub async fn list(state: &AppState, page: PageRequestDto) -> Result<PageDto<TokenDto>, UmtError> {
    let page = state.access.list_tokens(page.into()).await?;
    Ok(PageDto {
        items: page.items.into_iter().map(TokenDto::from).collect(),
        total: page.total,
    })
}

pub async fn create(
    state: &AppState,
    input: CreateTokenInput,
) -> Result<CreatedTokenDto, UmtError> {
    let created = state
        .access
        .create_token(&input.name, input.description, input.expires_at)
        .await?;

    record_event(
        state.audit.as_ref(),
        AuditKind::TokenCreated,
        AuditOutcome::Allowed,
        Some(created.token.id),
        None,
        None,
        None,
    )
    .await;

    Ok(CreatedTokenDto {
        token: TokenDto::from(created.token),
        plaintext: created.plaintext,
    })
}

pub async fn update(state: &AppState, input: UpdateTokenInput) -> Result<TokenDto, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(&input.id, "Token")?;
    let token = state
        .access
        .update_token(token_id, &input.name, input.description, input.expires_at)
        .await?;
    Ok(TokenDto::from(token))
}

pub async fn set_status(
    state: &AppState,
    input: SetTokenStatusInput,
) -> Result<TokenDto, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(&input.id, "Token")?;
    let token = state
        .access
        .set_token_status(token_id, input.status)
        .await?;

    let kind = match input.status {
        TokenStatus::Disabled => Some(AuditKind::TokenDisabled),
        TokenStatus::Revoked => Some(AuditKind::TokenRevoked),
        TokenStatus::Active => None,
    };
    if let Some(kind) = kind {
        record_event(
            state.audit.as_ref(),
            kind,
            AuditOutcome::Allowed,
            Some(token_id),
            None,
            None,
            None,
        )
        .await;
    }

    Ok(TokenDto::from(token))
}

pub async fn delete(state: &AppState, id: &str) -> Result<bool, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(id, "Token")?;
    state.access.delete_token(token_id).await
}

/// 按 Token 生成可直接粘贴到 AI 客户端的 MCP 配置。
///
/// 明文取自创建时保存的加密副本，旧版本创建的 Token 没有副本，需要重新创建。
pub async fn mcp_config(state: &AppState, token_id: &str) -> Result<String, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(token_id, "Token")?;
    state.access.get_token(token_id).await?;
    let secret = state.access.token_secret(token_id).await?.ok_or_else(|| {
        UmtError::Validation("该 Token 创建于旧版本，无法获取密钥，请在访问控制中重新创建".into())
    })?;

    let config = crate::services::state::read_mcp_config(&state.repos).await?;
    let endpoint = format!("http://{}:{}/mcp", config.host, config.port);
    let value = serde_json::json!({
        "mcpServers": {
            "universal-mcp-terminal": {
                "transport": "streamable-http",
                "url": endpoint,
                "headers": {
                    "Authorization": format!("Bearer {secret}")
                }
            }
        }
    });
    serde_json::to_string_pretty(&value)
        .map_err(|_| UmtError::Internal("MCP 配置无法序列化".into()))
}

pub async fn list_grants(state: &AppState, token_id: &str) -> Result<Vec<GrantDto>, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(token_id, "Token")?;
    let grants = state.access.list_grants(token_id).await?;
    let timestamps = grant_timestamps(state, token_id).await?;

    let mut result = Vec::with_capacity(grants.len());
    for view in grants {
        let granted_at = timestamps.get(&view.connection.id).copied();
        result.push(to_grant_dto(state, view, granted_at).await?);
    }
    Ok(result)
}

pub async fn set_grant(state: &AppState, input: SetGrantInput) -> Result<GrantDto, UmtError> {
    let token_id = crate::services::parse_id::<TokenId>(&input.token_id, "Token")?;
    let connection_id = crate::services::parse_id::<ConnectionId>(&input.connection_id, "连接")?;
    let view = state
        .access
        .set_grant(token_id, connection_id, input.allowed)
        .await?;

    record_event(
        state.audit.as_ref(),
        AuditKind::GrantChanged,
        AuditOutcome::Allowed,
        Some(token_id),
        Some(connection_id),
        None,
        Some(if input.allowed {
            "grant_allowed".to_string()
        } else {
            "grant_denied".to_string()
        }),
    )
    .await;

    let granted_at = input.allowed.then(UnixMillis::now);
    to_grant_dto(state, view, granted_at).await
}

async fn to_grant_dto(
    state: &AppState,
    view: GrantView,
    granted_at: Option<UnixMillis>,
) -> Result<GrantDto, UmtError> {
    let has_secret = state.has_secret(view.connection.id).await?;
    Ok(GrantDto {
        connection: to_dto(state, &view.connection, has_secret).await?,
        allowed: view.allowed,
        granted_at: if view.allowed { granted_at } else { None },
    })
}

/// `GrantView` 不携带授予时间，这里补齐用于前端展示。
async fn grant_timestamps(
    state: &AppState,
    token_id: TokenId,
) -> Result<BTreeMap<ConnectionId, UnixMillis>, UmtError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT connection_id, granted_at FROM token_connection_grants WHERE token_id = ?",
    )
    .bind(token_id.to_string())
    .fetch_all(&state.pool)
    .await
    .map_err(|err| UmtError::Storage(err.to_string()))?;

    let mut timestamps = BTreeMap::new();
    for (connection_id, granted_at) in rows {
        let connection_id = crate::services::parse_id::<ConnectionId>(&connection_id, "连接")?;
        timestamps.insert(connection_id, UnixMillis(granted_at));
    }
    Ok(timestamps)
}
