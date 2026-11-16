//! 审计查询、CSV 导出与统一的事件写入辅助。

use umt_audit::AuditService;
use umt_core::{
    AuditId, AuditKind, AuditOutcome, AuditQuery, AuditRecord, ConnectionId, ExecutionState,
    SessionId, TokenId, UmtError, UnixMillis,
};
use umt_persistence::NewAuditEntry;

use crate::ipc::dto::{AuditDto, AuditQueryDto, ExportResultDto, PageDto};
use crate::services::state::AppState;

/// 导出上限，与 `AuditService::export_csv` 内部的硬上限保持一致。
const EXPORT_LIMIT: u32 = 10_000;

/// 记录一条审计事件。审计失败不阻断主流程，只记录告警。
pub(crate) async fn record_event(
    audit: &AuditService,
    kind: AuditKind,
    outcome: AuditOutcome,
    token_id: Option<TokenId>,
    connection_id: Option<ConnectionId>,
    session_id: Option<SessionId>,
    command: Option<String>,
) {
    let entry = NewAuditEntry {
        session_id,
        token_id,
        connection_id,
        kind,
        outcome,
        tool_name: None,
        permission: None,
        command,
        working_dir: None,
        started_at: UnixMillis::now(),
        duration_ms: None,
        state: match outcome {
            AuditOutcome::Allowed => ExecutionState::Succeeded,
            AuditOutcome::Denied | AuditOutcome::Failed => ExecutionState::Failed,
        },
        exit_code: None,
        stdout_excerpt: None,
        stderr_excerpt: None,
        error: None,
        redacted: false,
    };
    if let Err(err) = audit.record(entry).await {
        tracing::warn!(code = err.code(), "写入审计事件失败");
    }
}

pub async fn query(state: &AppState, query: AuditQueryDto) -> Result<PageDto<AuditDto>, UmtError> {
    let query = to_query(&query)?;
    let page = state.audit.query(&query).await?;
    Ok(PageDto {
        items: page.items.into_iter().map(to_dto).collect(),
        total: page.total,
    })
}

pub async fn get(state: &AppState, id: i64) -> Result<AuditDto, UmtError> {
    let record = state.audit.get(AuditId(id)).await?;
    Ok(to_dto(record))
}

pub async fn export(state: &AppState, query: AuditQueryDto) -> Result<ExportResultDto, UmtError> {
    let mut query = to_query(&query)?;
    query.offset = 0;
    query.limit = EXPORT_LIMIT;

    let (csv, rows) = state.audit.export_csv(&query, EXPORT_LIMIT).await?;
    let dir = state.export_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| UmtError::Storage(format!("无法创建导出目录：{err}")))?;
    let path = dir.join(format!("audit-{}.csv", UnixMillis::now().0));
    tokio::fs::write(&path, csv)
        .await
        .map_err(|err| UmtError::Storage(format!("无法写入导出文件：{err}")))?;

    Ok(ExportResultDto {
        path: path_string(&path),
        rows,
    })
}

fn to_query(query: &AuditQueryDto) -> Result<AuditQuery, UmtError> {
    let page: umt_core::PageRequest = query.page.into();
    let token_id = query
        .token_id
        .as_deref()
        .map(|value| crate::services::parse_id(value, "Token"))
        .transpose()?;
    let connection_id = query
        .connection_id
        .as_deref()
        .map(|value| crate::services::parse_id(value, "连接"))
        .transpose()?;
    let session_id = query
        .session_id
        .as_deref()
        .map(|value| crate::services::parse_id(value, "会话"))
        .transpose()?;

    Ok(AuditQuery {
        token_id,
        connection_id,
        session_id,
        kind: query.kind,
        outcome: query.outcome,
        from: query.from,
        to: query.to,
        offset: page.offset,
        limit: page.limit,
    })
}

pub(crate) fn to_dto(record: AuditRecord) -> AuditDto {
    AuditDto {
        id: record.id.0,
        session_id: record.session_id.map(|value| value.to_string()),
        token_id: record.token_id.map(|value| value.to_string()),
        connection_id: record.connection_id.map(|value| value.to_string()),
        kind: record.kind,
        outcome: record.outcome,
        tool_name: record.tool_name,
        permission: record.permission,
        command: record.command,
        working_dir: record.working_dir,
        started_at: record.started_at,
        duration_ms: record.duration_ms,
        state: execution_state(&record.state).to_string(),
        exit_code: record.exit_code,
        stdout_excerpt: record.stdout_excerpt,
        stderr_excerpt: record.stderr_excerpt,
        error: record.error,
        redacted: record.redacted,
    }
}

fn execution_state(state: &ExecutionState) -> &'static str {
    match state {
        ExecutionState::Sent => "sent",
        ExecutionState::Running => "running",
        ExecutionState::Succeeded => "succeeded",
        ExecutionState::Failed => "failed",
        ExecutionState::TimedOut => "timed_out",
        ExecutionState::Unknown => "unknown",
    }
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
