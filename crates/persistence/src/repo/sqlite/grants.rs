//! Token 与连接授权关系的 SQLite 实现。

use async_trait::async_trait;
use sqlx::SqlitePool;
use umt_core::{ConnectionId, TokenId, UnixMillis};

use super::support::{
    RowExt, backend, bool_from_int, connection_from_row, is_foreign_key_violation, parse_id,
};
use crate::error::PersistenceError;
use crate::models::GrantView;
use crate::repo::GrantRepo;

/// 授权关系的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteGrantRepo {
    pool: SqlitePool,
}

impl SqliteGrantRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GrantRepo for SqliteGrantRepo {
    async fn set(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        allowed: bool,
    ) -> Result<(), PersistenceError> {
        let result = sqlx::query(
            "INSERT INTO token_connection_grants (token_id, connection_id, allowed, granted_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(token_id, connection_id) DO UPDATE SET allowed = excluded.allowed",
        )
        .bind(token_id.to_string())
        .bind(connection_id.to_string())
        .bind(i64::from(allowed))
        .bind(UnixMillis::now().0)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if is_foreign_key_violation(&err) => Err(PersistenceError::not_found(
                "Token 或连接",
                format!("{token_id}/{connection_id}"),
            )),
            Err(err) => Err(backend(err)),
        }
    }

    async fn is_allowed(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
    ) -> Result<bool, PersistenceError> {
        let allowed: Option<i64> = sqlx::query_scalar(
            "SELECT allowed FROM token_connection_grants \
             WHERE token_id = ? AND connection_id = ?",
        )
        .bind(token_id.to_string())
        .bind(connection_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(allowed.is_some_and(bool_from_int))
    }

    async fn revoke_all_for_token(&self, token_id: TokenId) -> Result<u64, PersistenceError> {
        let done = sqlx::query("DELETE FROM token_connection_grants WHERE token_id = ?")
            .bind(token_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(done.rows_affected())
    }

    async fn revoke_all_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<u64, PersistenceError> {
        let done = sqlx::query("DELETE FROM token_connection_grants WHERE connection_id = ?")
            .bind(connection_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(done.rows_affected())
    }

    async fn list_for_token(&self, token_id: TokenId) -> Result<Vec<GrantView>, PersistenceError> {
        // 以 connections 为左表，未设置授权的连接也要返回（allowed = false）。
        let rows = sqlx::query(
            "SELECT c.id, c.name, c.protocol_id, c.description, c.tags_json, c.notes, \
             c.enabled, c.created_at, c.updated_at, c.revision, \
             COALESCE(g.allowed, 0) AS allowed \
             FROM connections c \
             LEFT JOIN token_connection_grants g \
             ON g.connection_id = c.id AND g.token_id = ? \
             ORDER BY c.created_at ASC, c.id ASC",
        )
        .bind(token_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;

        rows.iter()
            .map(|row| {
                Ok(GrantView {
                    connection: connection_from_row(row)?,
                    allowed: bool_from_int(row.int("allowed")?),
                })
            })
            .collect()
    }

    async fn list_tokens_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Vec<TokenId>, PersistenceError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT token_id FROM token_connection_grants \
             WHERE connection_id = ? AND allowed = 1 ORDER BY token_id ASC",
        )
        .bind(connection_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;

        rows.into_iter()
            .map(|raw| parse_id::<TokenId>(raw, "Token"))
            .collect()
    }
}
