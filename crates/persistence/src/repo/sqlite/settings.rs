//! 键值设置仓库的 SQLite 实现。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::support::backend;
use crate::error::PersistenceError;
use crate::repo::SettingsRepo;

/// 键值设置的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteSettingsRepo {
    pool: SqlitePool,
}

impl SqliteSettingsRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettingsRepo for SqliteSettingsRepo {
    async fn get(&self, key: &str) -> Result<Option<String>, PersistenceError> {
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        Ok(value)
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), PersistenceError> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn all(&self) -> Result<BTreeMap<String, String>, PersistenceError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM settings ORDER BY key ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(backend)?;
        Ok(rows.into_iter().collect())
    }
}
