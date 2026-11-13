//! SQLite 连接池与迁移执行。CONTRACT-FROZEN: T3
//!
//! 只负责「把库打开到可用状态」：连接参数、建库、跑迁移。
//! 业务逻辑（启动恢复、默认数据等）由上层负责，不在 `connect` 内完成。

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;

use crate::error::PersistenceError;

/// SQLite 连接池句柄。
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// 打开（必要时创建）数据库并执行 `migrations/` 下的全部迁移。
    ///
    /// 连接参数：WAL、`foreign_keys=ON`、`busy_timeout = 5s`、自动建库。
    pub async fn connect(path: &Path) -> Result<Self, PersistenceError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(PersistenceError::backend)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(PersistenceError::backend)?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(PersistenceError::backend)?;

        Ok(Self { pool })
    }

    /// 供 T6 的仓库实现取用。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_runs_migrations_and_enables_foreign_keys() {
        let path =
            std::env::temp_dir().join(format!("umt-persistence-{}.db", uuid::Uuid::new_v4()));
        let db = Database::connect(&path).await.expect("打开数据库应成功");

        let mut tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(db.pool())
                .await
                .expect("查询表清单");
        tables.retain(|name| !name.starts_with('_') && !name.starts_with("sqlite_"));

        assert_eq!(
            tables,
            vec![
                "audit_events",
                "connection_secrets",
                "connections",
                "sessions",
                "settings",
                "ssh_host_keys",
                "token_connection_grants",
                "tokens",
            ],
            "v1 表结构不应包含 import_log"
        );

        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .expect("读取 PRAGMA foreign_keys");
        assert_eq!(foreign_keys, 1);

        drop(db);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
