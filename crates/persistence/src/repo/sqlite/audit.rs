//! 审计仓库的 SQLite 实现（只追加，不修改、不删除）。

use async_trait::async_trait;
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use umt_core::{
    AuditId, AuditOutcome, AuditQuery, AuditRecord, ConnectionId, ExecutionState, Page, SessionId,
    TokenId, UnixMillis,
};

use super::support::{
    RowExt, audit_kind_from_str, audit_kind_to_str, audit_outcome_from_str, audit_outcome_to_str,
    backend, bool_from_int, count_to_u64, execution_state_from_str, execution_state_to_str, narrow,
    parse_id,
};
use crate::error::PersistenceError;
use crate::models::NewAuditEntry;
use crate::repo::AuditRepo;

const AUDIT_COLUMNS: &str = "id, session_id, token_id, connection_id, kind, command, working_dir, \
                             started_at, duration_ms, status, exit_code, stdout_excerpt, \
                             stderr_excerpt, error, redacted";

/// 审计事件的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteAuditRepo {
    pool: SqlitePool,
}

impl SqliteAuditRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// 审计结果与执行状态的联合编码分隔符。
///
/// 冻结的 v1 建表没有 `outcome` 列，`status` 是唯一的状态文本列，
/// 因此把「outcome|state」写入该列；读写都封装在本模块，对外仍暴露两个独立字段。
const STATUS_SEPARATOR: char = '|';

fn encode_status(outcome: AuditOutcome, state: ExecutionState) -> String {
    format!(
        "{}{STATUS_SEPARATOR}{}",
        audit_outcome_to_str(outcome),
        execution_state_to_str(state)
    )
}

fn decode_status(raw: &str) -> Result<(AuditOutcome, ExecutionState), PersistenceError> {
    let (outcome, state) = raw
        .split_once(STATUS_SEPARATOR)
        .ok_or_else(|| PersistenceError::backend("审计状态格式非法"))?;
    Ok((
        audit_outcome_from_str(outcome)?,
        execution_state_from_str(state)?,
    ))
}

fn audit_from_row(row: &SqliteRow) -> Result<AuditRecord, PersistenceError> {
    let (outcome, state) = decode_status(&row.text("status")?)?;
    Ok(AuditRecord {
        id: AuditId(row.int("id")?),
        session_id: row
            .opt_text("session_id")?
            .map(|raw| parse_id::<SessionId>(raw, "会话"))
            .transpose()?,
        token_id: row
            .opt_text("token_id")?
            .map(|raw| parse_id::<TokenId>(raw, "Token"))
            .transpose()?,
        connection_id: row
            .opt_text("connection_id")?
            .map(|raw| parse_id::<ConnectionId>(raw, "连接"))
            .transpose()?,
        kind: audit_kind_from_str(&row.text("kind")?)?,
        outcome,
        command: row.opt_text("command")?,
        working_dir: row.opt_text("working_dir")?,
        started_at: UnixMillis(row.int("started_at")?),
        duration_ms: row.opt_int("duration_ms")?.map(narrow::<u64>).transpose()?,
        state,
        exit_code: row.opt_int("exit_code")?.map(narrow::<i32>).transpose()?,
        stdout_excerpt: row.opt_text("stdout_excerpt")?,
        stderr_excerpt: row.opt_text("stderr_excerpt")?,
        error: row.opt_text("error")?,
        redacted: bool_from_int(row.int("redacted")?),
    })
}

/// 追加全部过滤条件（AND 组合，时间闭区间）。
fn push_audit_filters(builder: &mut QueryBuilder<Sqlite>, query: &AuditQuery) {
    if let Some(token_id) = query.token_id {
        builder
            .push(" AND token_id = ")
            .push_bind(token_id.to_string());
    }
    if let Some(connection_id) = query.connection_id {
        builder
            .push(" AND connection_id = ")
            .push_bind(connection_id.to_string());
    }
    if let Some(session_id) = query.session_id {
        builder
            .push(" AND session_id = ")
            .push_bind(session_id.to_string());
    }
    if let Some(kind) = query.kind {
        builder
            .push(" AND kind = ")
            .push_bind(audit_kind_to_str(kind));
    }
    if let Some(outcome) = query.outcome {
        // `status` 存的是「outcome|state」，取分隔符前的部分做精确匹配。
        builder
            .push(" AND substr(status, 1, instr(status, '|') - 1) = ")
            .push_bind(audit_outcome_to_str(outcome));
    }
    if let Some(from) = query.from {
        builder.push(" AND started_at >= ").push_bind(from.0);
    }
    if let Some(to) = query.to {
        builder.push(" AND started_at <= ").push_bind(to.0);
    }
}

#[async_trait]
impl AuditRepo for SqliteAuditRepo {
    async fn append(&self, entry: NewAuditEntry) -> Result<AuditId, PersistenceError> {
        let done = sqlx::query(
            "INSERT INTO audit_events \
             (session_id, token_id, connection_id, kind, command, working_dir, \
              started_at, duration_ms, status, exit_code, stdout_excerpt, stderr_excerpt, \
              error, redacted) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.session_id.map(|id| id.to_string()))
        .bind(entry.token_id.map(|id| id.to_string()))
        .bind(entry.connection_id.map(|id| id.to_string()))
        .bind(audit_kind_to_str(entry.kind))
        .bind(entry.command)
        .bind(entry.working_dir)
        .bind(entry.started_at.0)
        .bind(entry.duration_ms.map(narrow_bind_u64))
        .bind(encode_status(entry.outcome, entry.state))
        .bind(entry.exit_code)
        .bind(entry.stdout_excerpt)
        .bind(entry.stderr_excerpt)
        .bind(entry.error)
        .bind(i64::from(entry.redacted))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(AuditId(done.last_insert_rowid()))
    }

    async fn query(&self, query: &AuditQuery) -> Result<Page<AuditRecord>, PersistenceError> {
        let mut count_builder =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM audit_events WHERE 1 = 1");
        push_audit_filters(&mut count_builder, query);
        let total: i64 = count_builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;

        let limit = query.limit_or_default();
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {AUDIT_COLUMNS} FROM audit_events WHERE 1 = 1"
        ));
        push_audit_filters(&mut builder, query);
        builder
            .push(" ORDER BY started_at DESC, id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(query.offset);
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        let items = rows
            .iter()
            .map(audit_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::new(items, count_to_u64(total)?))
    }

    async fn get(&self, id: AuditId) -> Result<Option<AuditRecord>, PersistenceError> {
        let row = sqlx::query(
            "SELECT id, session_id, token_id, connection_id, kind, command, \
             working_dir, started_at, duration_ms, status, exit_code, stdout_excerpt, \
             stderr_excerpt, error, redacted FROM audit_events WHERE id = ?",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(audit_from_row).transpose()
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;
        count_to_u64(total)
    }
}

/// `duration_ms` 存为 INTEGER，写入前把 `u64` 收窄为 `i64`。
fn narrow_bind_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
