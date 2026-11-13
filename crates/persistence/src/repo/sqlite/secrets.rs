//! 连接密文仓库的 SQLite 实现。

use async_trait::async_trait;
use sqlx::SqlitePool;
use umt_core::ConnectionId;

use super::support::{RowExt, backend, is_foreign_key_violation, narrow};
use crate::error::PersistenceError;
use crate::models::SecretBlob;
use crate::repo::SecretRepo;

/// 连接敏感配置（密文）的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteSecretRepo {
    pool: SqlitePool,
}

impl SqliteSecretRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SecretRepo for SqliteSecretRepo {
    async fn put(
        &self,
        connection_id: ConnectionId,
        secret: SecretBlob,
    ) -> Result<(), PersistenceError> {
        let result = sqlx::query(
            "INSERT INTO connection_secrets (connection_id, ciphertext, nonce, key_version) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(connection_id) DO UPDATE SET \
             ciphertext = excluded.ciphertext, nonce = excluded.nonce, \
             key_version = excluded.key_version",
        )
        .bind(connection_id.to_string())
        .bind(secret.ciphertext)
        .bind(secret.nonce)
        .bind(secret.key_version)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if is_foreign_key_violation(&err) => Err(PersistenceError::not_found(
                "连接",
                connection_id.to_string(),
            )),
            Err(err) => Err(backend(err)),
        }
    }

    async fn get(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Option<SecretBlob>, PersistenceError> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce, key_version FROM connection_secrets WHERE connection_id = ?",
        )
        .bind(connection_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;

        row.as_ref()
            .map(|row| {
                Ok(SecretBlob {
                    ciphertext: row.blob("ciphertext")?,
                    nonce: row.blob("nonce")?,
                    key_version: narrow::<u32>(row.int("key_version")?)?,
                })
            })
            .transpose()
    }

    async fn delete(&self, connection_id: ConnectionId) -> Result<bool, PersistenceError> {
        let done = sqlx::query("DELETE FROM connection_secrets WHERE connection_id = ?")
            .bind(connection_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }
}
