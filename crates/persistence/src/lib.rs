//! 持久化：SQLite 连接池、迁移、仓库 trait 与内存实现。
//!
//! 契约（建表 SQL / 仓库 trait / 内存实现）由 T3 冻结；
//! sqlx 的具体仓库实现由 T6 填充，不要在本 crate 之外复制这些 trait。
//!
//! 独占写入范围：`crates/persistence/**`。
// CONTRACT-FROZEN: T3

#![deny(unsafe_code)]

pub mod db;
pub mod error;
pub mod models;
pub mod repo;

pub use db::Database;
pub use error::PersistenceError;
pub use models::{
    ConnectionFilter, GrantView, HostKeyRecord, NewAuditEntry, SecretBlob, SessionFilter,
    TokenRecord,
};
pub use repo::memory::{
    InMemoryAuditRepo, InMemoryConnectionRepo, InMemoryGrantRepo, InMemoryHostKeyRepo,
    InMemoryRepos, InMemorySecretRepo, InMemorySessionRepo, InMemorySettingsRepo,
    InMemoryTokenRepo,
};
pub use repo::{
    AuditRepo, ConnectionRepo, GrantRepo, HostKeyRepo, SecretRepo, SessionRepo, SettingsRepo,
    TokenRepo,
};
