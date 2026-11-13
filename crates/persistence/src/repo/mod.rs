//! 仓库契约。CONTRACT-FROZEN: T3
//!
//! 约定：
//! - 所有 trait 都要求 `Send + Sync`，方法一律 `async`，错误统一 `PersistenceError`。
//! - `get` 类方法用 `Option` 表达「不存在」，只有「必须存在」的写操作才返回 `NotFound`。
//! - 时间一律 `umt_core::UnixMillis`（Unix 毫秒），ID 一律 core 的强类型 ID。
//! - `Page<T>.total` 是**过滤后**的总条数，而不是当前页长度。
//!
//! 具体 sqlx 实现由 T6 提供；本模块只冻结签名与语义。

pub mod memory;

use async_trait::async_trait;
use std::collections::BTreeMap;
use umt_core::{
    AuditId, AuditQuery, AuditRecord, Connection, ConnectionId, Page, PageRequest, SessionId,
    SessionInfo, SessionState, Token, TokenId, TokenStatus, UnixMillis,
};

use crate::error::PersistenceError;
use crate::models::{
    ConnectionFilter, GrantView, HostKeyRecord, NewAuditEntry, SecretBlob, SessionFilter,
    TokenRecord,
};

/// 连接聚合的增删改查。
#[async_trait]
pub trait ConnectionRepo: Send + Sync {
    /// 新建连接。`id` 重复返回 `Conflict`，`name` 重复返回 `UniqueViolation`。
    async fn create(&self, connection: Connection) -> Result<(), PersistenceError>;

    /// 按 ID 读取；不存在返回 `Ok(None)`。
    async fn get(&self, id: ConnectionId) -> Result<Option<Connection>, PersistenceError>;

    /// 分页查询，`total` 为过滤后的总数。
    async fn list(&self, filter: &ConnectionFilter) -> Result<Page<Connection>, PersistenceError>;

    /// 乐观锁更新。`connection.revision` 会被忽略，只有 `expected_revision` 与库中版本
    /// 一致时才写入，并把库中版本置为 `expected_revision + 1`；返回写入后的记录。
    /// 版本不符返回 `Conflict`，记录不存在返回 `NotFound`。
    async fn update(
        &self,
        connection: Connection,
        expected_revision: u32,
    ) -> Result<Connection, PersistenceError>;

    /// 删除；返回是否真的删掉了记录。
    async fn delete(&self, id: ConnectionId) -> Result<bool, PersistenceError>;

    /// 全部连接数量（不带筛选）。
    async fn count(&self) -> Result<u64, PersistenceError>;

    /// 启用/停用。会同样递增 `revision` 并刷新 `updated_at`，避免绕过乐观锁。
    async fn set_enabled(&self, id: ConnectionId, enabled: bool) -> Result<(), PersistenceError>;
}

/// 连接敏感配置（密文）存储。一个连接最多一条。
#[async_trait]
pub trait SecretRepo: Send + Sync {
    /// 写入或覆盖。`connection_id` 必须已存在。
    async fn put(
        &self,
        connection_id: ConnectionId,
        secret: SecretBlob,
    ) -> Result<(), PersistenceError>;

    async fn get(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Option<SecretBlob>, PersistenceError>;

    /// 返回是否删除了已有密文。
    async fn delete(&self, connection_id: ConnectionId) -> Result<bool, PersistenceError>;
}

/// 访问 Token 存储。
#[async_trait]
pub trait TokenRepo: Send + Sync {
    /// 新建 Token。`id` 重复返回 `Conflict`，`prefix` 重复返回 `UniqueViolation`。
    async fn create(&self, record: TokenRecord) -> Result<(), PersistenceError>;

    /// 读取含 HMAC 的完整记录（仅内部鉴权使用）。
    async fn get_record(&self, id: TokenId) -> Result<Option<TokenRecord>, PersistenceError>;

    /// 读取对外可见信息（不含 HMAC）。
    async fn get(&self, id: TokenId) -> Result<Option<Token>, PersistenceError>;

    /// 按创建时间倒序分页。
    async fn list(&self, page: PageRequest) -> Result<Page<Token>, PersistenceError>;

    async fn set_status(&self, id: TokenId, status: TokenStatus) -> Result<(), PersistenceError>;

    /// 更新名称、描述与有效期（状态变更走 `set_status`）。
    ///
    /// 默认实现返回错误，具体的仓库实现应当覆盖它。
    async fn update_metadata(
        &self,
        id: TokenId,
        name: &str,
        description: Option<&str>,
        expires_at: Option<UnixMillis>,
    ) -> Result<(), PersistenceError> {
        let _ = (id, name, description, expires_at);
        Err(PersistenceError::InvalidData(
            "当前 Token 仓库不支持元数据更新".into(),
        ))
    }

    /// 刷新最近使用时间。
    async fn touch_last_used(&self, id: TokenId, at: UnixMillis) -> Result<(), PersistenceError>;

    /// 按明文前缀查找（前缀唯一）。
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<TokenRecord>, PersistenceError>;

    async fn count(&self) -> Result<u64, PersistenceError>;

    async fn delete(&self, id: TokenId) -> Result<bool, PersistenceError>;

    /// 保存 Token 明文的加密副本，用于在本机生成 MCP 配置。
    async fn put_secret(&self, id: TokenId, secret: SecretBlob) -> Result<(), PersistenceError>;

    /// 读取 Token 明文的加密副本；旧版本创建的 Token 没有此记录。
    async fn get_secret(&self, id: TokenId) -> Result<Option<SecretBlob>, PersistenceError>;
}

/// Token 与 Connection 的多对多授权。
#[async_trait]
pub trait GrantRepo: Send + Sync {
    /// 设置授权位。`allowed = false` 写入一行显式拒绝记录（而非删除）。
    async fn set(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        allowed: bool,
    ) -> Result<(), PersistenceError>;

    /// 是否允许访问。只有存在且 `allowed = 1` 才返回 true。
    async fn is_allowed(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
    ) -> Result<bool, PersistenceError>;

    /// 清空该 Token 的全部授权记录，返回影响行数。
    async fn revoke_all_for_token(&self, token_id: TokenId) -> Result<u64, PersistenceError>;

    /// 清空该连接的全部授权记录，返回影响行数。
    async fn revoke_all_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<u64, PersistenceError>;

    /// 返回**全部现有连接**及其对该 Token 的授权状态（未授权即 `allowed = false`），
    /// 供前端渲染授权矩阵。按连接创建时间升序。
    async fn list_for_token(&self, token_id: TokenId) -> Result<Vec<GrantView>, PersistenceError>;

    /// 返回被允许访问该连接的 Token 列表（升序，去重）。
    async fn list_tokens_for_connection(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Vec<TokenId>, PersistenceError>;
}

/// 会话记录。
#[async_trait]
pub trait SessionRepo: Send + Sync {
    /// 写入新会话。`id` 重复返回 `Conflict`。
    async fn create(&self, session: &SessionInfo) -> Result<(), PersistenceError>;

    async fn get(&self, id: SessionId) -> Result<Option<SessionInfo>, PersistenceError>;

    /// 更新状态机字段。状态迁移是否合法由 Session Manager 判定，本层不拦截；
    /// 记录不存在返回 `NotFound`。
    async fn update_state(
        &self,
        id: SessionId,
        state: SessionState,
        ended_at: Option<UnixMillis>,
        close_reason: Option<String>,
    ) -> Result<(), PersistenceError>;

    /// 活跃会话（`Connecting` / `Connected` / `Running`），按启动时间倒序分页。
    async fn list_active(&self, page: PageRequest) -> Result<Page<SessionInfo>, PersistenceError>;

    /// 某 Token 的会话历史，按启动时间倒序分页。
    async fn list_by_token(
        &self,
        token_id: TokenId,
        page: PageRequest,
    ) -> Result<Page<SessionInfo>, PersistenceError>;

    /// 条件查询会话历史，按启动时间倒序分页。
    async fn list_history(
        &self,
        filter: &SessionFilter,
    ) -> Result<Page<SessionInfo>, PersistenceError>;

    async fn count_active(&self) -> Result<u64, PersistenceError>;

    /// 把该 Token 的所有活跃会话置为 `Closed`（`ended_at = now`，写入 `close_reason`），
    /// 返回受影响条数。用于 Token 撤销/停用后的联动终止。
    async fn close_all_by_token(
        &self,
        token_id: TokenId,
        reason: &str,
    ) -> Result<u64, PersistenceError>;
}

/// 审计事件（只追加，不修改、不删除）。
#[async_trait]
pub trait AuditRepo: Send + Sync {
    /// 追加一条记录，返回自增主键。
    async fn append(&self, entry: NewAuditEntry) -> Result<AuditId, PersistenceError>;

    /// 条件查询；结果按 `started_at` 倒序（同一时刻按 id 倒序），`total` 为过滤后总数。
    async fn query(&self, query: &AuditQuery) -> Result<Page<AuditRecord>, PersistenceError>;

    async fn get(&self, id: AuditId) -> Result<Option<AuditRecord>, PersistenceError>;

    /// 全部审计记录数（不带筛选）。
    async fn count(&self) -> Result<u64, PersistenceError>;
}

/// 键值设置。
#[async_trait]
pub trait SettingsRepo: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, PersistenceError>;

    /// 写入或覆盖。
    async fn set(&self, key: &str, value: &str) -> Result<(), PersistenceError>;

    /// 全部设置，按 key 升序。
    async fn all(&self) -> Result<BTreeMap<String, String>, PersistenceError>;
}

/// SSH Host Key 信任库。
#[async_trait]
pub trait HostKeyRepo: Send + Sync {
    async fn get(&self, endpoint: &str) -> Result<Option<HostKeyRecord>, PersistenceError>;

    /// 按 `endpoint` 插入或整行覆盖。
    async fn upsert(&self, record: HostKeyRecord) -> Result<(), PersistenceError>;

    /// 删除已信任的指纹，返回是否真的删除了记录。
    async fn delete(&self, endpoint: &str) -> Result<bool, PersistenceError>;

    /// 全部已信任指纹，按 trusted_at 倒序。
    async fn list(&self) -> Result<Vec<HostKeyRecord>, PersistenceError>;
}

pub mod sqlite;
