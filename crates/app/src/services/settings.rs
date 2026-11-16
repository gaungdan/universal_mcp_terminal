//! 系统设置与首页概览。

use std::time::Duration;

use umt_core::{AuditQuery, PageRequest, UmtError};
use umt_persistence::{ConnectionFilter, ConnectionRepo, SessionRepo, SettingsRepo, TokenRepo};
use umt_session_manager::{ExecutionMode, SessionLimits};

use crate::ipc::dto::{OverviewDto, SettingsDto};
use crate::services::audit::to_dto;
use crate::services::mcp;
use crate::services::state::{
    AppState, DEFAULT_SESSION_TIMEOUT_SECS, KEY_EXECUTION_MODE, KEY_LOG_LEVEL,
    KEY_REQUIRE_CONFIRMATION, KEY_SESSION_TIMEOUT,
};

const LOG_LEVELS: [&str; 4] = ["info", "debug", "warn", "error"];
const RECENT_AUDIT_LIMIT: u32 = 5;

pub async fn get(state: &AppState) -> Result<SettingsDto, UmtError> {
    let settings = state.repos.settings.all().await?;
    let get = |key: &str, fallback: &str| {
        settings
            .get(key)
            .cloned()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback.to_string())
    };

    Ok(SettingsDto {
        mcp: mcp::status(state).await,
        security: crate::ipc::dto::SecuritySettingsDto {
            default_execution_mode: get(KEY_EXECUTION_MODE, "Restricted"),
            session_timeout_secs: settings
                .get(KEY_SESSION_TIMEOUT)
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|secs| *secs > 0)
                .unwrap_or(DEFAULT_SESSION_TIMEOUT_SECS),
            require_confirmation_for_high_risk: settings
                .get(KEY_REQUIRE_CONFIRMATION)
                .map(|value| value != "false")
                .unwrap_or(true),
        },
        log_level: get(KEY_LOG_LEVEL, "info"),
    })
}

pub async fn update(
    state: &AppState,
    input: crate::ipc::dto::UpdateSettingsInput,
) -> Result<SettingsDto, UmtError> {
    let mode = input
        .security
        .default_execution_mode
        .parse::<ExecutionMode>()
        .map_err(|_| {
            UmtError::Validation("默认执行模式必须是 ReadOnly / Restricted / FullAccess".into())
        })?;
    if input.security.session_timeout_secs == 0 {
        return Err(UmtError::Validation("Session 超时时间必须大于 0".into()));
    }
    let log_level = input.log_level.trim().to_ascii_lowercase();
    if !LOG_LEVELS.contains(&log_level.as_str()) {
        return Err(UmtError::Validation(
            "日志级别必须是 info / debug / warn / error".into(),
        ));
    }

    state
        .set_setting(KEY_EXECUTION_MODE, &input.security.default_execution_mode)
        .await?;
    state
        .set_setting(
            KEY_SESSION_TIMEOUT,
            &input.security.session_timeout_secs.to_string(),
        )
        .await?;
    state
        .set_setting(
            KEY_REQUIRE_CONFIRMATION,
            if input.security.require_confirmation_for_high_risk {
                "true"
            } else {
                "false"
            },
        )
        .await?;
    state.set_setting(KEY_LOG_LEVEL, &log_level).await?;

    // 全局执行模式仅保留为兼容设置，MCP 命令权限以 Token 为准。
    let _ = mode;
    state.sessions.set_limits(SessionLimits {
        idle_timeout: Duration::from_secs(input.security.session_timeout_secs),
        ..SessionLimits::default()
    });

    get(state).await
}

pub async fn overview(state: &AppState) -> Result<OverviewDto, UmtError> {
    let connection_count = state.repos.connections.count().await?;
    let enabled_connection_count = state
        .repos
        .connections
        .list(&ConnectionFilter {
            enabled: Some(true),
            page: PageRequest::new(0, 1),
            ..Default::default()
        })
        .await?
        .total;
    let token_count = state.repos.tokens.count().await?;
    let active_session_count = state.repos.sessions.count_active().await?;

    let recent = state
        .audit
        .query(&AuditQuery {
            limit: RECENT_AUDIT_LIMIT,
            ..Default::default()
        })
        .await?;

    Ok(OverviewDto {
        connection_count,
        enabled_connection_count,
        token_count,
        active_session_count,
        mcp: mcp::status(state).await,
        recent_audits: recent.items.into_iter().map(to_dto).collect(),
    })
}
