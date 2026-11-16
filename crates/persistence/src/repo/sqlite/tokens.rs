//! Token 仓库的 SQLite 实现。

use async_trait::async_trait;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use umt_core::{Page, PageRequest, Token, TokenId, TokenPermission, TokenStatus, UnixMillis};

use super::support::{
    RowExt, backend, bool_from_int, count_to_u64, is_foreign_key_violation, is_unique_violation,
    narrow, parse_id, token_permission_from_str, token_permission_to_str, token_status_from_str,
};
use crate::error::PersistenceError;
use crate::models::{SecretBlob, TokenRecord};
use crate::repo::TokenRepo;

/// 访问 Token 的 SQLite 仓库。
#[derive(Debug, Clone)]
pub struct SqliteTokenRepo {
    pool: SqlitePool,
}

impl SqliteTokenRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn fetch_record(&self, id: TokenId) -> Result<Option<TokenRecord>, PersistenceError> {
        let row = sqlx::query(
            "SELECT id, name, description, prefix, hmac, expires_at, created_at, last_used_at, \
             status, permission, allow_interactive FROM tokens WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(record_from_row).transpose()
    }
}

fn token_from_row(row: &SqliteRow) -> Result<Token, PersistenceError> {
    Ok(Token {
        id: parse_id::<TokenId>(row.text("id")?, "Token")?,
        name: row.text("name")?,
        description: row.opt_text("description")?,
        prefix: row.text("prefix")?,
        status: token_status_from_str(&row.text("status")?)?,
        permission: token_permission_from_str(&row.text("permission")?)?,
        allow_interactive: bool_from_int(row.int("allow_interactive")?),
        expires_at: row.opt_int("expires_at")?.map(UnixMillis),
        created_at: UnixMillis(row.int("created_at")?),
        last_used_at: row.opt_int("last_used_at")?.map(UnixMillis),
    })
}

fn record_from_row(row: &SqliteRow) -> Result<TokenRecord, PersistenceError> {
    Ok(TokenRecord {
        token: token_from_row(row)?,
        hmac: row.text("hmac")?,
    })
}

/// 分页结果的公共收尾：`total` 为过滤后总数。
fn paginate_tokens(items: Vec<Token>, total: u64, page: PageRequest) -> Page<Token> {
    let page = page.normalized();
    let start = (page.offset as usize).min(items.len());
    let end = start.saturating_add(page.limit as usize).min(items.len());
    Page::new(items[start..end].to_vec(), total)
}

#[async_trait]
impl TokenRepo for SqliteTokenRepo {
    async fn create(&self, record: TokenRecord) -> Result<(), PersistenceError> {
        let id = record.token.id.to_string();
        let mut tx = self.pool.begin().await.map_err(backend)?;

        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM tokens WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        if existing.is_some() {
            return Err(PersistenceError::conflict("Token ID 已存在"));
        }

        let result = sqlx::query(
            "INSERT INTO tokens \
             (id, name, description, prefix, hmac, expires_at, created_at, last_used_at, status, \
              permission, allow_interactive) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(record.token.name.as_str())
        .bind(record.token.description.as_deref())
        .bind(record.token.prefix.as_str())
        .bind(record.hmac.as_str())
        .bind(record.token.expires_at.map(|at| at.0))
        .bind(record.token.created_at.0)
        .bind(record.token.last_used_at.map(|at| at.0))
        .bind(record.token.status.as_str())
        .bind(token_permission_to_str(record.token.permission))
        .bind(i64::from(record.token.allow_interactive))
        .execute(&mut *tx)
        .await;

        match result {
            Ok(_) => {
                tx.commit().await.map_err(backend)?;
                Ok(())
            }
            Err(err) if is_unique_violation(&err) => {
                Err(PersistenceError::unique_violation("Token 前缀已存在"))
            }
            Err(err) => Err(backend(err)),
        }
    }

    async fn get_record(&self, id: TokenId) -> Result<Option<TokenRecord>, PersistenceError> {
        self.fetch_record(id).await
    }

    async fn get(&self, id: TokenId) -> Result<Option<Token>, PersistenceError> {
        Ok(self.fetch_record(id).await?.map(|record| record.token))
    }

    async fn list(&self, page: PageRequest) -> Result<Page<Token>, PersistenceError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens")
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;

        let page = page.normalized();
        let rows = sqlx::query(
            "SELECT id, name, description, prefix, hmac, expires_at, created_at, last_used_at, \
             status, permission, allow_interactive FROM tokens \
             ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let items = rows
            .iter()
            .map(token_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(paginate_tokens(items, count_to_u64(total)?, page))
    }

    async fn set_status(&self, id: TokenId, status: TokenStatus) -> Result<(), PersistenceError> {
        let done = sqlx::query("UPDATE tokens SET status = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("Token", id.to_string()));
        }
        Ok(())
    }

    async fn update_metadata(
        &self,
        id: TokenId,
        name: &str,
        description: Option<&str>,
        expires_at: Option<UnixMillis>,
    ) -> Result<(), PersistenceError> {
        let done =
            sqlx::query("UPDATE tokens SET name = ?, description = ?, expires_at = ? WHERE id = ?")
                .bind(name)
                .bind(description)
                .bind(expires_at.map(|at| at.0))
                .bind(id.to_string())
                .execute(&self.pool)
                .await
                .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("Token", id.to_string()));
        }
        Ok(())
    }

    async fn set_permissions(
        &self,
        id: TokenId,
        permission: TokenPermission,
        allow_interactive: bool,
    ) -> Result<(), PersistenceError> {
        let done =
            sqlx::query("UPDATE tokens SET permission = ?, allow_interactive = ? WHERE id = ?")
                .bind(token_permission_to_str(permission))
                .bind(i64::from(allow_interactive))
                .bind(id.to_string())
                .execute(&self.pool)
                .await
                .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("Token", id.to_string()));
        }
        Ok(())
    }

    async fn touch_last_used(&self, id: TokenId, at: UnixMillis) -> Result<(), PersistenceError> {
        let done = sqlx::query("UPDATE tokens SET last_used_at = ? WHERE id = ?")
            .bind(at.0)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(PersistenceError::not_found("Token", id.to_string()));
        }
        Ok(())
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<TokenRecord>, PersistenceError> {
        let row = sqlx::query(
            "SELECT id, name, description, prefix, hmac, expires_at, created_at, last_used_at, \
             status, permission, allow_interactive FROM tokens WHERE prefix = ?",
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(record_from_row).transpose()
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens")
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;
        count_to_u64(total)
    }

    async fn delete(&self, id: TokenId) -> Result<bool, PersistenceError> {
        let done = sqlx::query("DELETE FROM tokens WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn put_secret(&self, id: TokenId, secret: SecretBlob) -> Result<(), PersistenceError> {
        let result = sqlx::query(
            "INSERT INTO token_secrets (token_id, ciphertext, nonce, key_version) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(token_id) DO UPDATE SET \
             ciphertext = excluded.ciphertext, nonce = excluded.nonce, \
             key_version = excluded.key_version",
        )
        .bind(id.to_string())
        .bind(secret.ciphertext)
        .bind(secret.nonce)
        .bind(secret.key_version)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if is_foreign_key_violation(&err) => {
                Err(PersistenceError::not_found("Token", id.to_string()))
            }
            Err(err) => Err(backend(err)),
        }
    }

    async fn get_secret(&self, id: TokenId) -> Result<Option<SecretBlob>, PersistenceError> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce, key_version FROM token_secrets WHERE token_id = ?",
        )
        .bind(id.to_string())
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
}
