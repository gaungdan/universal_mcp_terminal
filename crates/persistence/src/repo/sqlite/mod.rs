//! sqlx SQLite 版的全部仓库实现。
//!
//! 全部查询使用运行时 API（`sqlx::query` / `query_scalar` / `QueryBuilder`），
//! 不依赖编译期 `DATABASE_URL`，也不使用 `query!` 宏。

mod audit;
mod connections;
mod grants;
mod host_keys;
mod secrets;
mod sessions;
mod settings;
mod support;
mod tokens;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use sqlx::SqlitePool;

pub use audit::SqliteAuditRepo;
pub use connections::SqliteConnectionRepo;
pub use grants::SqliteGrantRepo;
pub use host_keys::SqliteHostKeyRepo;
pub use secrets::SqliteSecretRepo;
pub use sessions::SqliteSessionRepo;
pub use settings::SqliteSettingsRepo;
pub use tokens::SqliteTokenRepo;

/// 一次构造全部 8 个 SQLite 仓库，供 App 层直接注入。
#[derive(Debug, Clone)]
pub struct SqliteRepos {
    pub connections: Arc<SqliteConnectionRepo>,
    pub secrets: Arc<SqliteSecretRepo>,
    pub tokens: Arc<SqliteTokenRepo>,
    pub grants: Arc<SqliteGrantRepo>,
    pub sessions: Arc<SqliteSessionRepo>,
    pub audit: Arc<SqliteAuditRepo>,
    pub settings: Arc<SqliteSettingsRepo>,
    pub host_keys: Arc<SqliteHostKeyRepo>,
}

impl SqliteRepos {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            connections: Arc::new(SqliteConnectionRepo::new(pool.clone())),
            secrets: Arc::new(SqliteSecretRepo::new(pool.clone())),
            tokens: Arc::new(SqliteTokenRepo::new(pool.clone())),
            grants: Arc::new(SqliteGrantRepo::new(pool.clone())),
            sessions: Arc::new(SqliteSessionRepo::new(pool.clone())),
            audit: Arc::new(SqliteAuditRepo::new(pool.clone())),
            settings: Arc::new(SqliteSettingsRepo::new(pool.clone())),
            host_keys: Arc::new(SqliteHostKeyRepo::new(pool)),
        }
    }
}
