//! 连接仓库的 SQLite 实现。

use async_trait::async_trait;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use umt_core::{Connection, ConnectionId, Page, UnixMillis};

use super::support::{
    CONNECTION_COLUMNS, backend, connection_from_row, count_to_u64, int_from_bool,
    is_unique_violation, narrow,
};
use crate::error::PersistenceError;
use crate::models::ConnectionFilter;
use crate::repo::ConnectionRepo;

/// 连接聚合的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteConnectionRepo {
    pool: SqlitePool,
}

impl SqliteConnectionRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// `search` 对 name、description、notes、tags 做大小写不敏感包含匹配。
///
/// 与内存实现保持一致使用 Rust 侧 Unicode 小写比较；SQLite 的 `LOWER` 只处理 ASCII。
fn matches_search(connection: &Connection, needle: &str) -> bool {
    connection.name.to_lowercase().contains(needle)
        || connection
            .description
            .as_ref()
            .is_some_and(|d| d.to_lowercase().contains(needle))
        || connection
            .notes
            .as_ref()
            .is_some_and(|n| n.to_lowercase().contains(needle))
        || connection
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(needle))
}

#[async_trait]
impl ConnectionRepo for SqliteConnectionRepo {
    async fn create(&self, connection: Connection) -> Result<(), PersistenceError> {
        let id = connection.id.to_string();
        let mut tx = self.pool.begin().await.map_err(backend)?;

        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM connections WHERE id = ?")
                .bind(id.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        if existing.is_some() {
            return Err(PersistenceError::conflict("连接 ID 已存在"));
        }

        let tags_json = serde_json::to_string(&connection.tags)
            .map_err(|_| PersistenceError::invalid_data("连接的 tags 无法序列化"))?;
        let result = sqlx::query(
            "INSERT INTO connections \
             (id, name, protocol_id, description, tags_json, notes, enabled, created_at, updated_at, revision) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(connection.name.as_str())
        .bind(connection.protocol_id.as_str())
        .bind(connection.description.as_deref())
        .bind(tags_json.as_str())
        .bind(connection.notes.as_deref())
        .bind(int_from_bool(connection.enabled))
        .bind(connection.created_at.0)
        .bind(connection.updated_at.0)
        .bind(connection.revision)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(_) => {
                tx.commit().await.map_err(backend)?;
                Ok(())
            }
            Err(err) if is_unique_violation(&err) => {
                Err(PersistenceError::unique_violation("连接名称已存在"))
            }
            Err(err) => Err(backend(err)),
        }
    }

    async fn get(&self, id: ConnectionId) -> Result<Option<Connection>, PersistenceError> {
        let row = sqlx::query(
            "SELECT id, name, protocol_id, description, tags_json, notes, enabled, created_at, \
             updated_at, revision FROM connections WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(connection_from_row).transpose()
    }

    async fn list(&self, filter: &ConnectionFilter) -> Result<Page<Connection>, PersistenceError> {
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {CONNECTION_COLUMNS} FROM connections WHERE 1 = 1"
        ));
        if let Some(protocol_id) = filter.protocol_id.as_deref() {
            builder.push(" AND protocol_id = ").push_bind(protocol_id);
        }
        if let Some(enabled) = filter.enabled {
            builder
                .push(" AND enabled = ")
                .push_bind(int_from_bool(enabled));
        }
        builder.push(" ORDER BY created_at ASC, id ASC");

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        let mut items = rows
            .iter()
            .map(connection_from_row)
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(search) = filter.search.as_ref().map(|s| s.to_lowercase()) {
            items.retain(|connection| matches_search(connection, &search));
        }

        let total = items.len() as u64;
        let page = filter.page.normalized();
        let start = (page.offset as usize).min(items.len());
        let end = start.saturating_add(page.limit as usize).min(items.len());
        Ok(Page::new(items[start..end].to_vec(), total))
    }

    async fn update(
        &self,
        connection: Connection,
        expected_revision: u32,
    ) -> Result<Connection, PersistenceError> {
        let id = connection.id.to_string();
        let mut tx = self.pool.begin().await.map_err(backend)?;

        let current: Option<i64> =
            sqlx::query_scalar("SELECT revision FROM connections WHERE id = ?")
                .bind(id.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        let Some(current) = current else {
            return Err(PersistenceError::not_found("连接", id));
        };
        if current != i64::from(expected_revision) {
            return Err(PersistenceError::conflict(format!(
                "连接 {} 版本号不符：期望 {expected_revision}，实际 {}",
                connection.id,
                narrow::<u32>(current)?
            )));
        }

        let next_revision = expected_revision.saturating_add(1);
        let tags_json = serde_json::to_string(&connection.tags)
            .map_err(|_| PersistenceError::invalid_data("连接的 tags 无法序列化"))?;
        let result = sqlx::query(
            "UPDATE connections SET name = ?, protocol_id = ?, description = ?, tags_json = ?, \
             notes = ?, enabled = ?, updated_at = ?, revision = ? \
             WHERE id = ? AND revision = ?",
        )
        .bind(connection.name.as_str())
        .bind(connection.protocol_id.as_str())
        .bind(connection.description.as_deref())
        .bind(tags_json.as_str())
        .bind(connection.notes.as_deref())
        .bind(int_from_bool(connection.enabled))
        .bind(connection.updated_at.0)
        .bind(next_revision)
        .bind(id.as_str())
        .bind(expected_revision)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(done) if done.rows_affected() == 0 => {
                // 事务内再确认一次，区分「已被删除」与「并发改版本」。
                let exists: Option<String> =
                    sqlx::query_scalar("SELECT id FROM connections WHERE id = ?")
                        .bind(id.as_str())
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(backend)?;
                if exists.is_none() {
                    Err(PersistenceError::not_found("连接", id))
                } else {
                    Err(PersistenceError::conflict(format!(
                        "连接 {} 版本号已变化",
                        connection.id
                    )))
                }
            }
            Ok(_) => {
                let row = sqlx::query(
                    "SELECT id, name, protocol_id, description, tags_json, notes, enabled, \
                     created_at, updated_at, revision FROM connections WHERE id = ?",
                )
                .bind(id.as_str())
                .fetch_one(&mut *tx)
                .await
                .map_err(backend)?;
                let updated = connection_from_row(&row)?;
                tx.commit().await.map_err(backend)?;
                Ok(updated)
            }
            Err(err) if is_unique_violation(&err) => {
                Err(PersistenceError::unique_violation("连接名称已存在"))
            }
            Err(err) => Err(backend(err)),
        }
    }

    async fn delete(&self, id: ConnectionId) -> Result<bool, PersistenceError> {
        let done = sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM connections")
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;
        count_to_u64(total)
    }

    async fn set_enabled(&self, id: ConnectionId, enabled: bool) -> Result<(), PersistenceError> {
        let done = sqlx::query(
            "UPDATE connections SET enabled = ?, updated_at = ?, revision = revision + 1 \
             WHERE id = ?",
        )
        .bind(int_from_bool(enabled))
        .bind(UnixMillis::now().0)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("连接", id.to_string()));
        }
        Ok(())
    }
}
