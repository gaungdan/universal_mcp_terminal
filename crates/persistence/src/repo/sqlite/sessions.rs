//! 会话仓库的 SQLite 实现。

use async_trait::async_trait;
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use umt_core::{Page, PageRequest, SessionId, SessionInfo, SessionState, TokenId, UnixMillis};

use super::support::{
    RowExt, backend, count_to_u64, is_unique_violation, narrow, parse_id, session_state_from_str,
    session_state_to_str,
};
use crate::error::PersistenceError;
use crate::models::SessionFilter;
use crate::repo::SessionRepo;

const SESSION_COLUMNS: &str = "id, token_id, connection_id, protocol_id, client_info, state, \
                               started_at, ended_at, close_reason, rows, cols";

/// 会话记录的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteSessionRepo {
    pool: SqlitePool,
}

impl SqliteSessionRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn session_from_row(row: &SqliteRow) -> Result<SessionInfo, PersistenceError> {
    Ok(SessionInfo {
        id: parse_id::<SessionId>(row.text("id")?, "会话")?,
        token_id: parse_id::<TokenId>(row.text("token_id")?, "Token")?,
        connection_id: parse_id::<umt_core::ConnectionId>(row.text("connection_id")?, "连接")?,
        protocol_id: row.text("protocol_id")?,
        client_info: row.opt_text("client_info")?,
        state: session_state_from_str(&row.text("state")?)?,
        started_at: UnixMillis(row.int("started_at")?),
        ended_at: row.opt_int("ended_at")?.map(UnixMillis),
        close_reason: row.opt_text("close_reason")?,
        rows: narrow::<u16>(row.opt_int("rows")?.unwrap_or(0))?,
        cols: narrow::<u16>(row.opt_int("cols")?.unwrap_or(0))?,
    })
}

/// 把 `SessionFilter` 的过滤条件追加到 WHERE 子句（全部为 AND）。
fn push_history_filters(builder: &mut QueryBuilder<Sqlite>, filter: &SessionFilter) {
    if let Some(token_id) = filter.token_id {
        builder
            .push(" AND token_id = ")
            .push_bind(token_id.to_string());
    }
    if let Some(connection_id) = filter.connection_id {
        builder
            .push(" AND connection_id = ")
            .push_bind(connection_id.to_string());
    }
    if let Some(state) = filter.state {
        builder
            .push(" AND state = ")
            .push_bind(session_state_to_str(state));
    }
    if let Some(from) = filter.from {
        builder.push(" AND started_at >= ").push_bind(from.0);
    }
    if let Some(to) = filter.to {
        builder.push(" AND started_at <= ").push_bind(to.0);
    }
}

fn paginate(items: Vec<SessionInfo>, total: u64, page: PageRequest) -> Page<SessionInfo> {
    let page = page.normalized();
    let start = (page.offset as usize).min(items.len());
    let end = start.saturating_add(page.limit as usize).min(items.len());
    Page::new(items[start..end].to_vec(), total)
}

#[async_trait]
impl SessionRepo for SqliteSessionRepo {
    async fn create(&self, session: &SessionInfo) -> Result<(), PersistenceError> {
        let result = sqlx::query(
            "INSERT INTO sessions \
             (id, token_id, connection_id, protocol_id, client_info, state, started_at, \
              ended_at, close_reason, rows, cols) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(session.token_id.to_string())
        .bind(session.connection_id.to_string())
        .bind(session.protocol_id.as_str())
        .bind(session.client_info.as_deref())
        .bind(session_state_to_str(session.state))
        .bind(session.started_at.0)
        .bind(session.ended_at.map(|at| at.0))
        .bind(session.close_reason.as_deref())
        .bind(session.rows)
        .bind(session.cols)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if is_unique_violation(&err) => {
                Err(PersistenceError::conflict("会话 ID 已存在"))
            }
            Err(err) => Err(backend(err)),
        }
    }

    async fn get(&self, id: SessionId) -> Result<Option<SessionInfo>, PersistenceError> {
        let row = sqlx::query(
            "SELECT id, token_id, connection_id, protocol_id, client_info, state, started_at, \
             ended_at, close_reason, rows, cols FROM sessions WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(session_from_row).transpose()
    }

    async fn update_state(
        &self,
        id: SessionId,
        state: SessionState,
        ended_at: Option<UnixMillis>,
        close_reason: Option<String>,
    ) -> Result<(), PersistenceError> {
        let done = sqlx::query(
            "UPDATE sessions SET state = ?, ended_at = ?, close_reason = ? WHERE id = ?",
        )
        .bind(session_state_to_str(state))
        .bind(ended_at.map(|at| at.0))
        .bind(close_reason)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("会话", id.to_string()));
        }
        Ok(())
    }

    async fn list_active(&self, page: PageRequest) -> Result<Page<SessionInfo>, PersistenceError> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions \
             WHERE state IN ('connecting', 'connected', 'running')",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(backend)?;

        let page = page.normalized();
        let rows = sqlx::query(
            "SELECT id, token_id, connection_id, protocol_id, client_info, state, started_at, \
             ended_at, close_reason, rows, cols FROM sessions \
             WHERE state IN ('connecting', 'connected', 'running') \
             ORDER BY started_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let items = rows
            .iter()
            .map(session_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(paginate(items, count_to_u64(total)?, page))
    }

    async fn list_by_token(
        &self,
        token_id: TokenId,
        page: PageRequest,
    ) -> Result<Page<SessionInfo>, PersistenceError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_id = ?")
            .bind(token_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;

        let page = page.normalized();
        let rows = sqlx::query(
            "SELECT id, token_id, connection_id, protocol_id, client_info, state, started_at, \
             ended_at, close_reason, rows, cols FROM sessions WHERE token_id = ? \
             ORDER BY started_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(token_id.to_string())
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let items = rows
            .iter()
            .map(session_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(paginate(items, count_to_u64(total)?, page))
    }

    async fn list_history(
        &self,
        filter: &SessionFilter,
    ) -> Result<Page<SessionInfo>, PersistenceError> {
        let mut count_builder =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM sessions WHERE 1 = 1");
        push_history_filters(&mut count_builder, filter);
        let total: i64 = count_builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;

        let page = filter.page.normalized();
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SESSION_COLUMNS} FROM sessions WHERE 1 = 1"
        ));
        push_history_filters(&mut builder, filter);
        builder
            .push(" ORDER BY started_at DESC, id DESC LIMIT ")
            .push_bind(page.limit)
            .push(" OFFSET ")
            .push_bind(page.offset);
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        let items = rows
            .iter()
            .map(session_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(paginate(items, count_to_u64(total)?, page))
    }

    async fn count_active(&self) -> Result<u64, PersistenceError> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions \
             WHERE state IN ('connecting', 'connected', 'running')",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(backend)?;
        count_to_u64(total)
    }

    async fn close_all_by_token(
        &self,
        token_id: TokenId,
        reason: &str,
    ) -> Result<u64, PersistenceError> {
        let done = sqlx::query(
            "UPDATE sessions SET state = 'closed', ended_at = ?, close_reason = ? \
             WHERE token_id = ? AND state IN ('connecting', 'connected', 'running')",
        )
        .bind(UnixMillis::now().0)
        .bind(reason)
        .bind(token_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(done.rows_affected())
    }
}
