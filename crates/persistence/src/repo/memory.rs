//! 全部仓库的内存实现，供其他 crate 并行写单元测试。
//!
//! 语义与 sqlx 实现保持一致（版本号递增、唯一冲突、审计排序与分页等），
//! 但**不保证**并发隔离级别与真实 SQLite 完全相同。

use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use umt_core::{
    AuditId, AuditQuery, AuditRecord, Connection, ConnectionId, Page, PageRequest, SessionId,
    SessionInfo, SessionState, Token, TokenId, TokenStatus, UnixMillis,
};

use super::{
    AuditRepo, ConnectionRepo, GrantRepo, HostKeyRepo, SecretRepo, SessionRepo, SettingsRepo,
    TokenRepo,
};
use crate::error::PersistenceError;
use crate::models::{
    ConnectionFilter, GrantView, HostKeyRecord, NewAuditEntry, SecretBlob, SessionFilter,
    TokenRecord,
};

/// 把已排序的切片按 `PageRequest` 切页，`total` 为传入集合的总长度。
fn paginate<T: Clone>(items: &[T], page: PageRequest) -> Page<T> {
    let page = page.normalized();
    let total = items.len() as u64;
    let start = (page.offset as usize).min(items.len());
    let end = start.saturating_add(page.limit as usize).min(items.len());
    Page::new(items[start..end].to_vec(), total)
}

// ---------------------------------------------------------------- connections

/// 内存版连接仓库。
#[derive(Debug, Default)]
pub struct InMemoryConnectionRepo {
    inner: RwLock<HashMap<ConnectionId, Connection>>,
}

impl InMemoryConnectionRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 内部快照：按创建时间升序（同一时刻按 id 升序）。
    /// 供 `InMemoryGrantRepo` 渲染授权矩阵使用。
    pub(crate) async fn snapshot(&self) -> Vec<Connection> {
        let mut items: Vec<Connection> = self.inner.read().await.values().cloned().collect();
        items.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.as_uuid().cmp(&b.id.as_uuid()))
        });
        items
    }
}

#[async_trait]
impl ConnectionRepo for InMemoryConnectionRepo {
    async fn create(&self, connection: Connection) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        if map.contains_key(&connection.id) {
            return Err(PersistenceError::conflict("连接 ID 已存在"));
        }
        if map.values().any(|c| c.name == connection.name) {
            return Err(PersistenceError::unique_violation("连接名称已存在"));
        }
        map.insert(connection.id, connection);
        Ok(())
    }

    async fn get(&self, id: ConnectionId) -> Result<Option<Connection>, PersistenceError> {
        Ok(self.inner.read().await.get(&id).cloned())
    }

    async fn list(&self, filter: &ConnectionFilter) -> Result<Page<Connection>, PersistenceError> {
        let search = filter.search.as_ref().map(|s| s.to_lowercase());
        let mut items: Vec<Connection> = self
            .inner
            .read()
            .await
            .values()
            .filter(|c| {
                filter
                    .protocol_id
                    .as_ref()
                    .is_none_or(|p| &c.protocol_id == p)
            })
            .filter(|c| filter.enabled.is_none_or(|e| c.enabled == e))
            .filter(|c| match &search {
                None => true,
                Some(needle) => {
                    c.name.to_lowercase().contains(needle)
                        || c.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(needle))
                        || c.notes
                            .as_ref()
                            .is_some_and(|n| n.to_lowercase().contains(needle))
                        || c.tags.iter().any(|t| t.to_lowercase().contains(needle))
                }
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.as_uuid().cmp(&b.id.as_uuid()))
        });
        Ok(paginate(&items, filter.page))
    }

    async fn update(
        &self,
        connection: Connection,
        expected_revision: u32,
    ) -> Result<Connection, PersistenceError> {
        let mut map = self.inner.write().await;
        let current = map
            .get(&connection.id)
            .ok_or_else(|| PersistenceError::not_found("连接", connection.id.to_string()))?;
        if current.revision != expected_revision {
            return Err(PersistenceError::conflict(format!(
                "连接 {} 版本号不符：期望 {expected_revision}，实际 {}",
                connection.id, current.revision
            )));
        }
        if map
            .values()
            .any(|c| c.id != connection.id && c.name == connection.name)
        {
            return Err(PersistenceError::unique_violation("连接名称已存在"));
        }
        let mut updated = connection;
        updated.revision = expected_revision.saturating_add(1);
        map.insert(updated.id, updated.clone());
        Ok(updated)
    }

    async fn delete(&self, id: ConnectionId) -> Result<bool, PersistenceError> {
        Ok(self.inner.write().await.remove(&id).is_some())
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        Ok(self.inner.read().await.len() as u64)
    }

    async fn set_enabled(&self, id: ConnectionId, enabled: bool) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        let connection = map
            .get_mut(&id)
            .ok_or_else(|| PersistenceError::not_found("连接", id.to_string()))?;
        connection.enabled = enabled;
        connection.revision = connection.revision.saturating_add(1);
        connection.updated_at = UnixMillis::now();
        Ok(())
    }
}

// ------------------------------------------------------------------- secrets

/// 内存版密文仓库。
#[derive(Debug, Default)]
pub struct InMemorySecretRepo {
    inner: RwLock<HashMap<ConnectionId, SecretBlob>>,
}

impl InMemorySecretRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecretRepo for InMemorySecretRepo {
    async fn put(
        &self,
        connection_id: ConnectionId,
        secret: SecretBlob,
    ) -> Result<(), PersistenceError> {
        self.inner.write().await.insert(connection_id, secret);
        Ok(())
    }

    async fn get(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Option<SecretBlob>, PersistenceError> {
        Ok(self.inner.read().await.get(&connection_id).cloned())
    }

    async fn delete(&self, connection_id: ConnectionId) -> Result<bool, PersistenceError> {
        Ok(self.inner.write().await.remove(&connection_id).is_some())
    }
}

// -------------------------------------------------------------------- tokens

/// 内存版 Token 仓库。
#[derive(Debug, Default)]
pub struct InMemoryTokenRepo {
    inner: RwLock<HashMap<TokenId, TokenRecord>>,
}

impl InMemoryTokenRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TokenRepo for InMemoryTokenRepo {
    async fn create(&self, record: TokenRecord) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        if map.contains_key(&record.token.id) {
            return Err(PersistenceError::conflict("Token ID 已存在"));
        }
        if map.values().any(|r| r.token.prefix == record.token.prefix) {
            return Err(PersistenceError::unique_violation("Token 前缀已存在"));
        }
        map.insert(record.token.id, record);
        Ok(())
    }

    async fn get_record(&self, id: TokenId) -> Result<Option<TokenRecord>, PersistenceError> {
        Ok(self.inner.read().await.get(&id).cloned())
    }

    async fn get(&self, id: TokenId) -> Result<Option<Token>, PersistenceError> {
        Ok(self.inner.read().await.get(&id).map(|r| r.token.clone()))
    }

    async fn list(&self, page: PageRequest) -> Result<Page<Token>, PersistenceError> {
        let mut items: Vec<Token> = self
            .inner
            .read()
            .await
            .values()
            .map(|r| r.token.clone())
            .collect();
        items.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.as_uuid().cmp(&a.id.as_uuid()))
        });
        Ok(paginate(&items, page))
    }

    async fn set_status(&self, id: TokenId, status: TokenStatus) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        let record = map
            .get_mut(&id)
            .ok_or_else(|| PersistenceError::not_found("Token", id.to_string()))?;
        record.token.status = status;
        Ok(())
    }

    async fn update_metadata(
        &self,
        id: TokenId,
        name: &str,
        description: Option<&str>,
        expires_at: Option<UnixMillis>,
    ) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        let record = map
            .get_mut(&id)
            .ok_or_else(|| PersistenceError::not_found("Token", id.to_string()))?;
        record.token.name = name.to_string();
        record.token.description = description.map(str::to_string);
        record.token.expires_at = expires_at;
        Ok(())
    }

    async fn touch_last_used(&self, id: TokenId, at: UnixMillis) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        let record = map
            .get_mut(&id)
            .ok_or_else(|| PersistenceError::not_found("Token", id.to_string()))?;
        record.token.last_used_at = Some(at);
        Ok(())
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<TokenRecord>, PersistenceError> {
        Ok(self
            .inner
            .read()
            .await
            .values()
            .find(|r| r.token.prefix == prefix)
            .cloned())
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        Ok(self.inner.read().await.len() as u64)
    }

    async fn delete(&self, id: TokenId) -> Result<bool, PersistenceError> {
        Ok(self.inner.write().await.remove(&id).is_some())
    }
}

// -------------------------------------------------------------------- grants

#[derive(Debug, Clone, Copy)]
struct GrantEntry {
    allowed: bool,
}

/// 内存版授权仓库。
///
/// 需要读取连接列表来渲染 `GrantView`，因此持有连接仓库的引用。
#[derive(Debug)]
pub struct InMemoryGrantRepo {
    inner: RwLock<HashMap<(TokenId, ConnectionId), GrantEntry>>,
    connections: Arc<InMemoryConnectionRepo>,
}

impl InMemoryGrantRepo {
    pub fn new(connections: Arc<InMemoryConnectionRepo>) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            connections,
        }
    }
}

#[async_trait]
impl GrantRepo for InMemoryGrantRepo {
    async fn set(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        allowed: bool,
    ) -> Result<(), PersistenceError> {
        self.inner
            .write()
            .await
            .insert((token_id, connection_id), GrantEntry { allowed });
        Ok(())
    }

    async fn is_allowed(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
    ) -> Result<bool, PersistenceError> {
        Ok(self
            .inner
            .read()
            .await
            .get(&(token_id, connection_id))
            .is_some_and(|entry| entry.allowed))
    }

    async fn revoke_all_for_token(&self, token_id: TokenId) -> Result<u64, PersistenceError> {
        let mut map = self.inner.write().await;
        let before = map.len();
        map.retain(|(tid, _), _| *tid != token_id);
        Ok((before - map.len()) as u64)
    }

    async fn revoke_all_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<u64, PersistenceError> {
        let mut map = self.inner.write().await;
        let before = map.len();
        map.retain(|(_, cid), _| *cid != connection_id);
        Ok((before - map.len()) as u64)
    }

    async fn list_for_token(&self, token_id: TokenId) -> Result<Vec<GrantView>, PersistenceError> {
        let connections = self.connections.snapshot().await;
        let map = self.inner.read().await;
        Ok(connections
            .into_iter()
            .map(|connection| {
                let allowed = map
                    .get(&(token_id, connection.id))
                    .is_some_and(|entry| entry.allowed);
                GrantView {
                    connection,
                    allowed,
                }
            })
            .collect())
    }

    async fn list_tokens_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Vec<TokenId>, PersistenceError> {
        let mut tokens: Vec<TokenId> = self
            .inner
            .read()
            .await
            .iter()
            .filter(|((_, cid), entry)| *cid == connection_id && entry.allowed)
            .map(|((tid, _), _)| *tid)
            .collect();
        tokens.sort();
        Ok(tokens)
    }
}

// ------------------------------------------------------------------ sessions

/// 内存版会话仓库。
#[derive(Debug, Default)]
pub struct InMemorySessionRepo {
    inner: RwLock<HashMap<SessionId, SessionInfo>>,
}

impl InMemorySessionRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 会话统一按启动时间倒序（同一时刻按 id 倒序）。
fn sort_sessions_desc(items: &mut [SessionInfo]) {
    items.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.id.as_uuid().cmp(&a.id.as_uuid()))
    });
}

#[async_trait]
impl SessionRepo for InMemorySessionRepo {
    async fn create(&self, session: &SessionInfo) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        if map.contains_key(&session.id) {
            return Err(PersistenceError::conflict("会话 ID 已存在"));
        }
        map.insert(session.id, session.clone());
        Ok(())
    }

    async fn get(&self, id: SessionId) -> Result<Option<SessionInfo>, PersistenceError> {
        Ok(self.inner.read().await.get(&id).cloned())
    }

    async fn update_state(
        &self,
        id: SessionId,
        state: SessionState,
        ended_at: Option<UnixMillis>,
        close_reason: Option<String>,
    ) -> Result<(), PersistenceError> {
        let mut map = self.inner.write().await;
        let session = map
            .get_mut(&id)
            .ok_or_else(|| PersistenceError::not_found("会话", id.to_string()))?;
        session.state = state;
        session.ended_at = ended_at;
        session.close_reason = close_reason;
        Ok(())
    }

    async fn list_active(&self, page: PageRequest) -> Result<Page<SessionInfo>, PersistenceError> {
        let mut items: Vec<SessionInfo> = self
            .inner
            .read()
            .await
            .values()
            .filter(|s| s.state.is_active())
            .cloned()
            .collect();
        sort_sessions_desc(&mut items);
        Ok(paginate(&items, page))
    }

    async fn list_by_token(
        &self,
        token_id: TokenId,
        page: PageRequest,
    ) -> Result<Page<SessionInfo>, PersistenceError> {
        let mut items: Vec<SessionInfo> = self
            .inner
            .read()
            .await
            .values()
            .filter(|s| s.token_id == token_id)
            .cloned()
            .collect();
        sort_sessions_desc(&mut items);
        Ok(paginate(&items, page))
    }

    async fn list_history(
        &self,
        filter: &SessionFilter,
    ) -> Result<Page<SessionInfo>, PersistenceError> {
        let mut items: Vec<SessionInfo> = self
            .inner
            .read()
            .await
            .values()
            .filter(|s| filter.token_id.is_none_or(|t| s.token_id == t))
            .filter(|s| filter.connection_id.is_none_or(|c| s.connection_id == c))
            .filter(|s| filter.state.is_none_or(|st| s.state == st))
            .filter(|s| filter.from.is_none_or(|from| s.started_at >= from))
            .filter(|s| filter.to.is_none_or(|to| s.started_at <= to))
            .cloned()
            .collect();
        sort_sessions_desc(&mut items);
        Ok(paginate(&items, filter.page))
    }

    async fn count_active(&self) -> Result<u64, PersistenceError> {
        Ok(self
            .inner
            .read()
            .await
            .values()
            .filter(|s| s.state.is_active())
            .count() as u64)
    }

    async fn close_all_by_token(
        &self,
        token_id: TokenId,
        reason: &str,
    ) -> Result<u64, PersistenceError> {
        let now = UnixMillis::now();
        let mut map = self.inner.write().await;
        let mut closed = 0u64;
        for session in map.values_mut() {
            if session.token_id == token_id && session.state.is_active() {
                session.state = SessionState::Closed;
                session.ended_at = Some(now);
                session.close_reason = Some(reason.to_string());
                closed += 1;
            }
        }
        Ok(closed)
    }
}

// --------------------------------------------------------------------- audit

#[derive(Debug, Default)]
struct AuditStore {
    next_id: i64,
    records: Vec<AuditRecord>,
}

/// 内存版审计仓库（只追加）。
#[derive(Debug, Default)]
pub struct InMemoryAuditRepo {
    inner: RwLock<AuditStore>,
}

impl InMemoryAuditRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AuditRepo for InMemoryAuditRepo {
    async fn append(&self, entry: NewAuditEntry) -> Result<AuditId, PersistenceError> {
        let mut store = self.inner.write().await;
        let id = AuditId(store.next_id);
        store.next_id += 1;
        store.records.push(AuditRecord {
            id,
            session_id: entry.session_id,
            token_id: entry.token_id,
            connection_id: entry.connection_id,
            kind: entry.kind,
            outcome: entry.outcome,
            command: entry.command,
            working_dir: entry.working_dir,
            started_at: entry.started_at,
            duration_ms: entry.duration_ms,
            state: entry.state,
            exit_code: entry.exit_code,
            stdout_excerpt: entry.stdout_excerpt,
            stderr_excerpt: entry.stderr_excerpt,
            error: entry.error,
            redacted: entry.redacted,
        });
        Ok(id)
    }

    async fn query(&self, query: &AuditQuery) -> Result<Page<AuditRecord>, PersistenceError> {
        let mut items: Vec<AuditRecord> = self
            .inner
            .read()
            .await
            .records
            .iter()
            .filter(|r| query.token_id.is_none_or(|t| r.token_id == Some(t)))
            .filter(|r| {
                query
                    .connection_id
                    .is_none_or(|c| r.connection_id == Some(c))
            })
            .filter(|r| query.session_id.is_none_or(|s| r.session_id == Some(s)))
            .filter(|r| query.kind.is_none_or(|k| r.kind == k))
            .filter(|r| query.outcome.is_none_or(|o| r.outcome == o))
            .filter(|r| query.from.is_none_or(|from| r.started_at >= from))
            .filter(|r| query.to.is_none_or(|to| r.started_at <= to))
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.id.0.cmp(&a.id.0))
        });
        let total = items.len() as u64;
        let start = (query.offset as usize).min(items.len());
        let end = start
            .saturating_add(query.limit_or_default() as usize)
            .min(items.len());
        Ok(Page::new(items[start..end].to_vec(), total))
    }

    async fn get(&self, id: AuditId) -> Result<Option<AuditRecord>, PersistenceError> {
        Ok(self
            .inner
            .read()
            .await
            .records
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }

    async fn count(&self) -> Result<u64, PersistenceError> {
        Ok(self.inner.read().await.records.len() as u64)
    }
}

// ------------------------------------------------------------------ settings

/// 内存版键值设置仓库。
#[derive(Debug, Default)]
pub struct InMemorySettingsRepo {
    inner: RwLock<BTreeMap<String, String>>,
}

impl InMemorySettingsRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SettingsRepo for InMemorySettingsRepo {
    async fn get(&self, key: &str) -> Result<Option<String>, PersistenceError> {
        Ok(self.inner.read().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), PersistenceError> {
        self.inner
            .write()
            .await
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn all(&self) -> Result<BTreeMap<String, String>, PersistenceError> {
        Ok(self.inner.read().await.clone())
    }
}

// ------------------------------------------------------------------ host keys

/// 内存版 SSH Host Key 信任库。
#[derive(Debug, Default)]
pub struct InMemoryHostKeyRepo {
    inner: RwLock<HashMap<String, HostKeyRecord>>,
}

impl InMemoryHostKeyRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl HostKeyRepo for InMemoryHostKeyRepo {
    async fn get(&self, endpoint: &str) -> Result<Option<HostKeyRecord>, PersistenceError> {
        Ok(self.inner.read().await.get(endpoint).cloned())
    }

    async fn upsert(&self, record: HostKeyRecord) -> Result<(), PersistenceError> {
        self.inner
            .write()
            .await
            .insert(record.endpoint.clone(), record);
        Ok(())
    }

    async fn delete(&self, endpoint: &str) -> Result<bool, PersistenceError> {
        Ok(self.inner.write().await.remove(endpoint).is_some())
    }

    async fn list(&self) -> Result<Vec<HostKeyRecord>, PersistenceError> {
        let mut records: Vec<HostKeyRecord> = self.inner.read().await.values().cloned().collect();
        records.sort_by_key(|record| std::cmp::Reverse(record.trusted_at));
        Ok(records)
    }
}

// ------------------------------------------------------------------ 组合入口

/// 一次性构造全部 8 个内存仓库。
///
/// 其他 crate 的单测只需 `let repos = InMemoryRepos::new();` 后各自取用字段。
#[derive(Debug, Clone)]
pub struct InMemoryRepos {
    pub connections: Arc<InMemoryConnectionRepo>,
    pub secrets: Arc<InMemorySecretRepo>,
    pub tokens: Arc<InMemoryTokenRepo>,
    pub grants: Arc<InMemoryGrantRepo>,
    pub sessions: Arc<InMemorySessionRepo>,
    pub audit: Arc<InMemoryAuditRepo>,
    pub settings: Arc<InMemorySettingsRepo>,
    pub host_keys: Arc<InMemoryHostKeyRepo>,
}

impl InMemoryRepos {
    pub fn new() -> Self {
        let connections = Arc::new(InMemoryConnectionRepo::new());
        Self {
            grants: Arc::new(InMemoryGrantRepo::new(Arc::clone(&connections))),
            connections,
            secrets: Arc::new(InMemorySecretRepo::new()),
            tokens: Arc::new(InMemoryTokenRepo::new()),
            sessions: Arc::new(InMemorySessionRepo::new()),
            audit: Arc::new(InMemoryAuditRepo::new()),
            settings: Arc::new(InMemorySettingsRepo::new()),
            host_keys: Arc::new(InMemoryHostKeyRepo::new()),
        }
    }
}

impl Default for InMemoryRepos {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_core::{AuditKind, AuditOutcome, ExecutionState};

    fn connection(name: &str, protocol: &str, created_at: i64) -> Connection {
        Connection {
            id: ConnectionId::new(),
            name: name.to_string(),
            protocol_id: protocol.to_string(),
            description: Some(format!("{name} 描述")),
            tags: vec!["prod".to_string()],
            notes: None,
            enabled: true,
            created_at: UnixMillis(created_at),
            updated_at: UnixMillis(created_at),
            revision: 1,
        }
    }

    fn token(name: &str, prefix: &str, created_at: i64) -> TokenRecord {
        TokenRecord {
            token: Token {
                id: TokenId::new(),
                name: name.to_string(),
                description: None,
                prefix: prefix.to_string(),
                status: TokenStatus::Active,
                expires_at: None,
                created_at: UnixMillis(created_at),
                last_used_at: None,
            },
            hmac: format!("hmac-{prefix}"),
        }
    }

    fn session(
        id: SessionId,
        token_id: TokenId,
        connection_id: ConnectionId,
        state: SessionState,
        started_at: i64,
    ) -> SessionInfo {
        SessionInfo {
            id,
            token_id,
            connection_id,
            protocol_id: "ssh".to_string(),
            client_info: None,
            state,
            started_at: UnixMillis(started_at),
            ended_at: None,
            close_reason: None,
            rows: 40,
            cols: 120,
        }
    }

    fn audit_entry(kind: AuditKind, token_id: Option<TokenId>, started_at: i64) -> NewAuditEntry {
        NewAuditEntry {
            session_id: None,
            token_id,
            connection_id: None,
            kind,
            outcome: AuditOutcome::Allowed,
            command: Some("ls".to_string()),
            working_dir: Some("/tmp".to_string()),
            started_at: UnixMillis(started_at),
            duration_ms: Some(5),
            state: ExecutionState::Succeeded,
            exit_code: Some(0),
            stdout_excerpt: None,
            stderr_excerpt: None,
            error: None,
            redacted: false,
        }
    }

    #[tokio::test]
    async fn connection_create_update_and_revision_conflict() {
        let repos = InMemoryRepos::new();
        let conn = connection("web", "ssh", 1_000);
        let id = conn.id;
        repos
            .connections
            .create(conn.clone())
            .await
            .expect("创建应成功");

        assert_eq!(repos.connections.count().await.expect("计数"), 1);
        assert_eq!(
            repos.connections.get(id).await.expect("读取"),
            Some(conn.clone())
        );

        // 重复 ID / 重复名称
        assert!(matches!(
            repos.connections.create(conn.clone()).await,
            Err(PersistenceError::Conflict(_))
        ));
        let mut dup_name = connection("web", "telnet", 1_001);
        dup_name.id = ConnectionId::new();
        assert!(matches!(
            repos.connections.create(dup_name).await,
            Err(PersistenceError::UniqueViolation(_))
        ));

        // 版本不符
        let mut edited = conn.clone();
        edited.name = "web-2".to_string();
        assert!(matches!(
            repos.connections.update(edited.clone(), 99).await,
            Err(PersistenceError::Conflict(_))
        ));

        let updated = repos
            .connections
            .update(edited.clone(), 1)
            .await
            .expect("版本正确应成功");
        assert_eq!(updated.revision, 2);
        assert_eq!(
            repos
                .connections
                .get(id)
                .await
                .expect("读取")
                .expect("应存在")
                .name,
            "web-2"
        );

        // set_enabled 也会推进 revision
        repos
            .connections
            .set_enabled(id, false)
            .await
            .expect("停用应成功");
        let after = repos
            .connections
            .get(id)
            .await
            .expect("读取")
            .expect("应存在");
        assert!(!after.enabled);
        assert_eq!(after.revision, 3);

        assert!(repos.connections.delete(id).await.expect("删除应成功"));
        assert!(!repos.connections.delete(id).await.expect("再删返回 false"));
        assert_eq!(
            repos.connections.get(id).await.expect("读取"),
            None,
            "删除后不可见"
        );
    }

    #[tokio::test]
    async fn connection_list_filters_and_paginates() {
        let repos = InMemoryRepos::new();
        repos
            .connections
            .create(connection("alpha", "ssh", 1_000))
            .await
            .expect("创建");
        repos
            .connections
            .create(connection("beta", "telnet", 2_000))
            .await
            .expect("创建");
        repos
            .connections
            .create(connection("gamma", "ssh", 3_000))
            .await
            .expect("创建");

        let ssh_only = ConnectionFilter {
            protocol_id: Some("ssh".to_string()),
            ..ConnectionFilter::default()
        };
        let page = repos.connections.list(&ssh_only).await.expect("查询");
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 2);

        let by_search = ConnectionFilter {
            search: Some("BET".to_string()),
            ..ConnectionFilter::default()
        };
        let page = repos.connections.list(&by_search).await.expect("查询");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].name, "beta");

        let paged = ConnectionFilter {
            page: PageRequest::new(1, 1),
            ..ConnectionFilter::default()
        };
        let page = repos.connections.list(&paged).await.expect("查询");
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "beta", "按创建时间升序，第二条是 beta");
    }

    #[tokio::test]
    async fn token_status_prefix_and_last_used() {
        let repos = InMemoryRepos::new();
        let record = token("claude", "umt_aaaa", 1_000);
        let id = record.token.id;
        repos.tokens.create(record.clone()).await.expect("创建");

        // 前缀唯一
        assert!(matches!(
            repos.tokens.create(token("other", "umt_aaaa", 2_000)).await,
            Err(PersistenceError::UniqueViolation(_))
        ));

        let found = repos
            .tokens
            .find_by_prefix("umt_aaaa")
            .await
            .expect("查询")
            .expect("应命中");
        assert_eq!(found.token.id, id);
        assert_eq!(found.hmac, "hmac-umt_aaaa");
        assert!(
            repos
                .tokens
                .find_by_prefix("umt_zzzz")
                .await
                .expect("查询")
                .is_none()
        );

        repos
            .tokens
            .set_status(id, TokenStatus::Revoked)
            .await
            .expect("改状态");
        assert_eq!(
            repos
                .tokens
                .get(id)
                .await
                .expect("读取")
                .expect("应存在")
                .status,
            TokenStatus::Revoked
        );

        let now = UnixMillis(9_999);
        repos.tokens.touch_last_used(id, now).await.expect("刷新");
        assert_eq!(
            repos
                .tokens
                .get(id)
                .await
                .expect("读取")
                .expect("应存在")
                .last_used_at,
            Some(now)
        );

        assert_eq!(repos.tokens.count().await.expect("计数"), 1);
        let page = repos
            .tokens
            .list(PageRequest::default())
            .await
            .expect("列表");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].prefix, "umt_aaaa");

        assert!(matches!(
            repos
                .tokens
                .set_status(TokenId::new(), TokenStatus::Disabled)
                .await,
            Err(PersistenceError::NotFound { .. })
        ));
        assert!(repos.tokens.delete(id).await.expect("删除"));
        assert_eq!(repos.tokens.count().await.expect("计数"), 0);
    }

    #[tokio::test]
    async fn grant_toggle_works_in_both_directions() {
        let repos = InMemoryRepos::new();
        let c1 = connection("c1", "ssh", 1_000);
        let c2 = connection("c2", "ssh", 2_000);
        let c1_id = c1.id;
        let c2_id = c2.id;
        repos.connections.create(c1).await.expect("创建");
        repos.connections.create(c2).await.expect("创建");

        let record = token("t1", "umt_tttt", 1_000);
        let token_id = record.token.id;
        repos.tokens.create(record).await.expect("创建");

        repos.grants.set(token_id, c1_id, true).await.expect("授权");
        repos
            .grants
            .set(token_id, c2_id, false)
            .await
            .expect("显式拒绝");

        assert!(
            repos
                .grants
                .is_allowed(token_id, c1_id)
                .await
                .expect("查询")
        );
        assert!(
            !repos
                .grants
                .is_allowed(token_id, c2_id)
                .await
                .expect("查询")
        );

        // 正向：按 token 看全部连接
        let views = repos.grants.list_for_token(token_id).await.expect("视图");
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].connection.id, c1_id);
        assert!(views[0].allowed);
        assert!(!views[1].allowed);

        // 反向：按连接看 token
        assert_eq!(
            repos
                .grants
                .list_tokens_for_connection(c1_id)
                .await
                .expect("查询"),
            vec![token_id]
        );
        assert!(
            repos
                .grants
                .list_tokens_for_connection(c2_id)
                .await
                .expect("查询")
                .is_empty()
        );

        // 关闭开关后双向都不再生效
        repos
            .grants
            .set(token_id, c1_id, false)
            .await
            .expect("关闭");
        assert!(
            !repos
                .grants
                .is_allowed(token_id, c1_id)
                .await
                .expect("查询")
        );
        assert!(
            repos
                .grants
                .list_tokens_for_connection(c1_id)
                .await
                .expect("查询")
                .is_empty()
        );

        // 重新打开并按 token 全量撤销
        repos.grants.set(token_id, c1_id, true).await.expect("授权");
        assert_eq!(
            repos
                .grants
                .revoke_all_for_token(token_id)
                .await
                .expect("撤销"),
            2
        );
        assert!(
            !repos
                .grants
                .is_allowed(token_id, c1_id)
                .await
                .expect("查询")
        );

        // 按连接撤销
        repos.grants.set(token_id, c2_id, true).await.expect("授权");
        assert_eq!(
            repos
                .grants
                .revoke_all_for_connection(c2_id)
                .await
                .expect("撤销"),
            1
        );
        assert!(
            !repos
                .grants
                .is_allowed(token_id, c2_id)
                .await
                .expect("查询")
        );
    }

    #[tokio::test]
    async fn audit_filters_by_condition_and_paginates() {
        let repos = InMemoryRepos::new();
        let token_a = TokenId::new();
        let token_b = TokenId::new();

        let first = repos
            .audit
            .append(audit_entry(
                AuditKind::CommandExecuted,
                Some(token_a),
                1_000,
            ))
            .await
            .expect("写入");
        repos
            .audit
            .append(audit_entry(AuditKind::SessionOpened, Some(token_b), 2_000))
            .await
            .expect("写入");
        repos
            .audit
            .append(audit_entry(
                AuditKind::CommandExecuted,
                Some(token_a),
                3_000,
            ))
            .await
            .expect("写入");

        assert_eq!(repos.audit.count().await.expect("计数"), 3);
        assert_eq!(
            repos
                .audit
                .get(first)
                .await
                .expect("读取")
                .expect("应存在")
                .kind,
            AuditKind::CommandExecuted
        );

        let by_kind = AuditQuery {
            kind: Some(AuditKind::CommandExecuted),
            ..AuditQuery::default()
        };
        let page = repos.audit.query(&by_kind).await.expect("查询");
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].started_at, UnixMillis(3_000), "按时间倒序");

        let by_token = AuditQuery {
            token_id: Some(token_b),
            ..AuditQuery::default()
        };
        let page = repos.audit.query(&by_token).await.expect("查询");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].started_at, UnixMillis(2_000));

        let by_time = AuditQuery {
            from: Some(UnixMillis(1_500)),
            to: Some(UnixMillis(2_500)),
            ..AuditQuery::default()
        };
        let page = repos.audit.query(&by_time).await.expect("查询");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].started_at, UnixMillis(2_000));

        let paged = AuditQuery {
            offset: 1,
            limit: 1,
            ..AuditQuery::default()
        };
        let page = repos.audit.query(&paged).await.expect("查询");
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].started_at, UnixMillis(2_000));
    }

    #[tokio::test]
    async fn close_all_by_token_closes_only_active_sessions_of_that_token() {
        let repos = InMemoryRepos::new();
        let token_a = TokenId::new();
        let token_b = TokenId::new();
        let connection_id = ConnectionId::new();

        let s1 = SessionId::new();
        let s2 = SessionId::new();
        let s3 = SessionId::new();
        let s4 = SessionId::new();
        repos
            .sessions
            .create(&session(
                s1,
                token_a,
                connection_id,
                SessionState::Running,
                1_000,
            ))
            .await
            .expect("创建");
        repos
            .sessions
            .create(&session(
                s2,
                token_a,
                connection_id,
                SessionState::Connected,
                2_000,
            ))
            .await
            .expect("创建");
        repos
            .sessions
            .create(&session(
                s3,
                token_a,
                connection_id,
                SessionState::Closed,
                3_000,
            ))
            .await
            .expect("创建");
        repos
            .sessions
            .create(&session(
                s4,
                token_b,
                connection_id,
                SessionState::Running,
                4_000,
            ))
            .await
            .expect("创建");

        assert_eq!(repos.sessions.count_active().await.expect("计数"), 3);

        let closed = repos
            .sessions
            .close_all_by_token(token_a, "token 已撤销")
            .await
            .expect("关闭");
        assert_eq!(closed, 2, "已是 Closed 的会话不计数");

        for id in [s1, s2] {
            let info = repos.sessions.get(id).await.expect("读取").expect("应存在");
            assert_eq!(info.state, SessionState::Closed);
            assert!(info.ended_at.is_some());
            assert_eq!(info.close_reason.as_deref(), Some("token 已撤销"));
        }
        let other = repos.sessions.get(s4).await.expect("读取").expect("应存在");
        assert_eq!(other.state, SessionState::Running, "其他 Token 不受影响");

        let active = repos
            .sessions
            .list_active(PageRequest::default())
            .await
            .expect("列表");
        assert_eq!(active.total, 1);
        assert_eq!(active.items[0].id, s4);

        let history = repos
            .sessions
            .list_by_token(token_a, PageRequest::default())
            .await
            .expect("列表");
        assert_eq!(history.total, 3);
        assert_eq!(history.items[0].id, s3, "按启动时间倒序");
    }

    #[tokio::test]
    async fn session_history_filter_and_update_state() {
        let repos = InMemoryRepos::new();
        let token_id = TokenId::new();
        let conn = ConnectionId::new();
        let s1 = SessionId::new();
        let s2 = SessionId::new();
        repos
            .sessions
            .create(&session(s1, token_id, conn, SessionState::Closed, 1_000))
            .await
            .expect("创建");
        repos
            .sessions
            .create(&session(s2, token_id, conn, SessionState::Running, 2_000))
            .await
            .expect("创建");

        let filter = SessionFilter {
            state: Some(SessionState::Closed),
            ..SessionFilter::default()
        };
        let page = repos.sessions.list_history(&filter).await.expect("查询");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, s1);

        repos
            .sessions
            .update_state(
                s2,
                SessionState::Closed,
                Some(UnixMillis(3_000)),
                Some("正常退出".into()),
            )
            .await
            .expect("更新");
        let info = repos.sessions.get(s2).await.expect("读取").expect("应存在");
        assert_eq!(info.state, SessionState::Closed);
        assert_eq!(info.ended_at, Some(UnixMillis(3_000)));
        assert_eq!(info.close_reason.as_deref(), Some("正常退出"));

        assert!(matches!(
            repos
                .sessions
                .update_state(SessionId::new(), SessionState::Closed, None, None)
                .await,
            Err(PersistenceError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn secrets_settings_and_host_keys() {
        let repos = InMemoryRepos::new();
        let id = ConnectionId::new();
        let secret = SecretBlob {
            ciphertext: vec![1, 2, 3],
            nonce: vec![9],
            key_version: 2,
        };
        repos.secrets.put(id, secret.clone()).await.expect("写入");
        assert_eq!(repos.secrets.get(id).await.expect("读取"), Some(secret));
        assert!(repos.secrets.delete(id).await.expect("删除"));
        assert!(!repos.secrets.delete(id).await.expect("再删返回 false"));

        repos.settings.set("mcp.port", "17321").await.expect("写入");
        repos.settings.set("ui.theme", "dark").await.expect("写入");
        repos.settings.set("mcp.port", "17322").await.expect("覆盖");
        assert_eq!(
            repos
                .settings
                .get("mcp.port")
                .await
                .expect("读取")
                .as_deref(),
            Some("17322")
        );
        let all = repos.settings.all().await.expect("全部");
        assert_eq!(all.len(), 2);
        assert_eq!(
            all.keys().cloned().collect::<Vec<_>>(),
            vec!["mcp.port".to_string(), "ui.theme".to_string()]
        );

        let record = HostKeyRecord {
            endpoint: "10.0.0.5:22".to_string(),
            host: "10.0.0.5".to_string(),
            port: 22,
            algorithm: Some("ssh-ed25519".to_string()),
            fingerprint: "SHA256:abc".to_string(),
            trusted_at: UnixMillis(1_000),
            trusted_by: None,
        };
        repos.host_keys.upsert(record.clone()).await.expect("写入");
        assert_eq!(
            repos.host_keys.get("10.0.0.5:22").await.expect("读取"),
            Some(record)
        );
        assert!(
            repos
                .host_keys
                .get("10.0.0.6:22")
                .await
                .expect("读取")
                .is_none()
        );
    }
}
