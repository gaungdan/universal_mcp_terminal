//! SQLite 仓库共用的工具：行读取、错误映射、ID 与枚举转换。

use std::str::FromStr;

use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use umt_core::{
    AuditKind, AuditOutcome, Connection, ConnectionId, ExecutionState, SessionState, TokenStatus,
};

use crate::error::PersistenceError;

/// 把 sqlx 错误折叠成持久化错误，避免上层直接依赖 sqlx 类型。
pub(super) fn backend(err: sqlx::Error) -> PersistenceError {
    PersistenceError::backend(err)
}

/// 是否为唯一约束冲突（含主键重复）。
pub(super) fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|e| e.is_unique_violation())
}

/// 是否为外键约束冲突（引用目标不存在）。
pub(super) fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|e| e.is_foreign_key_violation())
}

/// 从 TEXT 列还原强类型 ID。数据库内容异常时返回 `Backend`。
pub(super) fn parse_id<T>(raw: String, entity: &'static str) -> Result<T, PersistenceError>
where
    T: FromStr,
{
    raw.parse()
        .map_err(|_| PersistenceError::backend(format!("{entity} ID 格式非法")))
}

/// 把 INTEGER 列收窄到目标类型，超出范围视为存储层异常。
pub(super) fn narrow<T>(value: i64) -> Result<T, PersistenceError>
where
    T: TryFrom<i64>,
{
    T::try_from(value).map_err(|_| PersistenceError::backend("数据库整数值超出字段范围"))
}

/// COUNT(*) 结果转 `u64`。
pub(super) fn count_to_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::backend("数据库计数为负值"))
}

pub(super) fn bool_from_int(value: i64) -> bool {
    value != 0
}

pub(super) fn int_from_bool(value: bool) -> i64 {
    i64::from(value)
}

pub(super) fn token_status_from_str(raw: &str) -> Result<TokenStatus, PersistenceError> {
    match raw {
        "active" => Ok(TokenStatus::Active),
        "disabled" => Ok(TokenStatus::Disabled),
        "revoked" => Ok(TokenStatus::Revoked),
        other => Err(PersistenceError::backend(format!(
            "未知的 Token 状态：{other}"
        ))),
    }
}

pub(super) fn session_state_to_str(state: SessionState) -> &'static str {
    match state {
        SessionState::Connecting => "connecting",
        SessionState::Connected => "connected",
        SessionState::Running => "running",
        SessionState::Closed => "closed",
        SessionState::Failed => "failed",
        SessionState::Interrupted => "interrupted",
    }
}

pub(super) fn token_permission_to_str(permission: umt_core::TokenPermission) -> &'static str {
    permission.as_str()
}

pub(super) fn token_permission_from_str(
    raw: &str,
) -> Result<umt_core::TokenPermission, PersistenceError> {
    raw.parse::<umt_core::TokenPermission>()
        .map_err(PersistenceError::backend)
}

pub(super) fn session_state_from_str(raw: &str) -> Result<SessionState, PersistenceError> {
    match raw {
        "connecting" => Ok(SessionState::Connecting),
        "connected" => Ok(SessionState::Connected),
        "running" => Ok(SessionState::Running),
        "closed" => Ok(SessionState::Closed),
        "failed" => Ok(SessionState::Failed),
        "interrupted" => Ok(SessionState::Interrupted),
        other => Err(PersistenceError::backend(format!(
            "未知的会话状态：{other}"
        ))),
    }
}

pub(super) fn execution_state_to_str(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::Sent => "sent",
        ExecutionState::Running => "running",
        ExecutionState::Succeeded => "succeeded",
        ExecutionState::Failed => "failed",
        ExecutionState::TimedOut => "timed_out",
        ExecutionState::Unknown => "unknown",
    }
}

pub(super) fn execution_state_from_str(raw: &str) -> Result<ExecutionState, PersistenceError> {
    match raw {
        "sent" => Ok(ExecutionState::Sent),
        "running" => Ok(ExecutionState::Running),
        "succeeded" => Ok(ExecutionState::Succeeded),
        "failed" => Ok(ExecutionState::Failed),
        "timed_out" => Ok(ExecutionState::TimedOut),
        "unknown" => Ok(ExecutionState::Unknown),
        other => Err(PersistenceError::backend(format!(
            "未知的执行状态：{other}"
        ))),
    }
}

pub(super) fn audit_kind_to_str(kind: AuditKind) -> &'static str {
    match kind {
        AuditKind::SessionOpened => "session_opened",
        AuditKind::SessionClosed => "session_closed",
        AuditKind::CommandExecuted => "command_executed",
        AuditKind::FileRead => "file_read",
        AuditKind::InputSent => "input_sent",
        AuditKind::OutputRead => "output_read",
        AuditKind::PermissionDenied => "permission_denied",
        AuditKind::ConnectionCreated => "connection_created",
        AuditKind::ConnectionUpdated => "connection_updated",
        AuditKind::ConnectionDeleted => "connection_deleted",
        AuditKind::ConnectionTested => "connection_tested",
        AuditKind::TokenCreated => "token_created",
        AuditKind::TokenDisabled => "token_disabled",
        AuditKind::TokenRevoked => "token_revoked",
        AuditKind::GrantChanged => "grant_changed",
        AuditKind::McpServerStarted => "mcp_server_started",
        AuditKind::McpServerStopped => "mcp_server_stopped",
    }
}

pub(super) fn audit_kind_from_str(raw: &str) -> Result<AuditKind, PersistenceError> {
    match raw {
        "session_opened" => Ok(AuditKind::SessionOpened),
        "session_closed" => Ok(AuditKind::SessionClosed),
        "command_executed" => Ok(AuditKind::CommandExecuted),
        "file_read" => Ok(AuditKind::FileRead),
        "input_sent" => Ok(AuditKind::InputSent),
        "output_read" => Ok(AuditKind::OutputRead),
        "permission_denied" => Ok(AuditKind::PermissionDenied),
        "connection_created" => Ok(AuditKind::ConnectionCreated),
        "connection_updated" => Ok(AuditKind::ConnectionUpdated),
        "connection_deleted" => Ok(AuditKind::ConnectionDeleted),
        "connection_tested" => Ok(AuditKind::ConnectionTested),
        "token_created" => Ok(AuditKind::TokenCreated),
        "token_disabled" => Ok(AuditKind::TokenDisabled),
        "token_revoked" => Ok(AuditKind::TokenRevoked),
        "grant_changed" => Ok(AuditKind::GrantChanged),
        "mcp_server_started" => Ok(AuditKind::McpServerStarted),
        "mcp_server_stopped" => Ok(AuditKind::McpServerStopped),
        other => Err(PersistenceError::backend(format!(
            "未知的审计类型：{other}"
        ))),
    }
}

pub(super) fn audit_outcome_to_str(outcome: AuditOutcome) -> &'static str {
    match outcome {
        AuditOutcome::Allowed => "allowed",
        AuditOutcome::Denied => "denied",
        AuditOutcome::Failed => "failed",
    }
}

pub(super) fn audit_outcome_from_str(raw: &str) -> Result<AuditOutcome, PersistenceError> {
    match raw {
        "allowed" => Ok(AuditOutcome::Allowed),
        "denied" => Ok(AuditOutcome::Denied),
        "failed" => Ok(AuditOutcome::Failed),
        other => Err(PersistenceError::backend(format!(
            "未知的审计结果：{other}"
        ))),
    }
}

/// 行读取便捷方法，减少每列都写一遍 `map_err`。
pub(super) trait RowExt {
    fn text(&self, column: &str) -> Result<String, PersistenceError>;
    fn opt_text(&self, column: &str) -> Result<Option<String>, PersistenceError>;
    fn int(&self, column: &str) -> Result<i64, PersistenceError>;
    fn opt_int(&self, column: &str) -> Result<Option<i64>, PersistenceError>;
    fn blob(&self, column: &str) -> Result<Vec<u8>, PersistenceError>;
}

impl RowExt for SqliteRow {
    fn text(&self, column: &str) -> Result<String, PersistenceError> {
        self.try_get::<String, _>(column)
            .map_err(PersistenceError::backend)
    }

    fn opt_text(&self, column: &str) -> Result<Option<String>, PersistenceError> {
        self.try_get::<Option<String>, _>(column)
            .map_err(PersistenceError::backend)
    }

    fn int(&self, column: &str) -> Result<i64, PersistenceError> {
        self.try_get::<i64, _>(column)
            .map_err(PersistenceError::backend)
    }

    fn opt_int(&self, column: &str) -> Result<Option<i64>, PersistenceError> {
        self.try_get::<Option<i64>, _>(column)
            .map_err(PersistenceError::backend)
    }

    fn blob(&self, column: &str) -> Result<Vec<u8>, PersistenceError> {
        self.try_get::<Vec<u8>, _>(column)
            .map_err(PersistenceError::backend)
    }
}

/// 连接表的标准列清单，供连接仓库与授权视图复用。
pub(super) const CONNECTION_COLUMNS: &str = "id, name, protocol_id, description, tags_json, notes, enabled, created_at, updated_at, revision";

/// 把连接行映射为领域模型。`config_json` 不属于 `Connection`，读取时忽略。
pub(super) fn connection_from_row(row: &SqliteRow) -> Result<Connection, PersistenceError> {
    let tags_json = row.text("tags_json")?;
    let tags: Vec<String> = serde_json::from_str(&tags_json)
        .map_err(|_| PersistenceError::invalid_data("连接的 tags 不是合法 JSON 数组"))?;

    Ok(Connection {
        id: parse_id::<ConnectionId>(row.text("id")?, "连接")?,
        name: row.text("name")?,
        protocol_id: row.text("protocol_id")?,
        description: row.opt_text("description")?,
        tags,
        notes: row.opt_text("notes")?,
        enabled: bool_from_int(row.int("enabled")?),
        created_at: umt_core::UnixMillis(row.int("created_at")?),
        updated_at: umt_core::UnixMillis(row.int("updated_at")?),
        revision: narrow::<u32>(row.int("revision")?)?,
    })
}
