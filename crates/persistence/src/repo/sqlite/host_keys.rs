//! SSH Host Key 信任库的 SQLite 实现。

use async_trait::async_trait;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use umt_core::UnixMillis;

use super::support::{RowExt, backend, narrow};
use crate::error::PersistenceError;
use crate::models::HostKeyRecord;
use crate::repo::HostKeyRepo;

/// Host Key 信任库的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteHostKeyRepo {
    pool: SqlitePool,
}

impl SqliteHostKeyRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn host_key_from_row(row: &SqliteRow) -> Result<HostKeyRecord, PersistenceError> {
    Ok(HostKeyRecord {
        endpoint: row.text("endpoint")?,
        host: row.text("host")?,
        port: narrow::<u16>(row.int("port")?)?,
        algorithm: row.opt_text("algorithm")?,
        fingerprint: row.text("fingerprint")?,
        trusted_at: UnixMillis(row.int("trusted_at")?),
        trusted_by: row.opt_text("trusted_by")?,
    })
}

#[async_trait]
impl HostKeyRepo for SqliteHostKeyRepo {
    async fn get(&self, endpoint: &str) -> Result<Option<HostKeyRecord>, PersistenceError> {
        let row = sqlx::query(
            "SELECT endpoint, host, port, algorithm, fingerprint, trusted_at, trusted_by \
             FROM ssh_host_keys WHERE endpoint = ?",
        )
        .bind(endpoint)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(host_key_from_row).transpose()
    }

    async fn upsert(&self, record: HostKeyRecord) -> Result<(), PersistenceError> {
        sqlx::query(
            "INSERT INTO ssh_host_keys \
             (endpoint, host, port, algorithm, fingerprint, trusted_at, trusted_by) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(endpoint) DO UPDATE SET \
             host = excluded.host, port = excluded.port, algorithm = excluded.algorithm, \
             fingerprint = excluded.fingerprint, trusted_at = excluded.trusted_at, \
             trusted_by = excluded.trusted_by",
        )
        .bind(record.endpoint.as_str())
        .bind(record.host.as_str())
        .bind(record.port)
        .bind(record.algorithm.as_deref())
        .bind(record.fingerprint.as_str())
        .bind(record.trusted_at.0)
        .bind(record.trusted_by.as_deref())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }
}
