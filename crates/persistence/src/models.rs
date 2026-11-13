//! 仅持久化层使用的数据结构。CONTRACT-FROZEN: T3
//!
//! 领域模型一律复用 `umt_core`，这里只放「数据库才需要」的聚合形态，
//! 不要把 core 已有的结构再定义一遍。

use serde::{Deserialize, Serialize};
use umt_core::{
    AuditKind, AuditOutcome, Connection, ConnectionId, ExecutionState, PageRequest, SessionId,
    SessionState, Token, TokenId, UnixMillis,
};

/// 加密后的凭据密文。禁止序列化，禁止把内容写进日志。
#[derive(Clone, PartialEq, Eq)]
pub struct SecretBlob {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_version: u32,
}

// 手工 Debug：只暴露长度，避免密文/随机数被打印到日志。
impl std::fmt::Debug for SecretBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretBlob")
            .field("ciphertext_len", &self.ciphertext.len())
            .field("nonce_len", &self.nonce.len())
            .field("key_version", &self.key_version)
            .finish()
    }
}

/// Token 的持久化形态 = 对外可见信息 + 校验用 HMAC。
///
/// `hmac` 只允许在持久化层与访问控制层内部流转，不得通过 IPC 返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRecord {
    pub token: Token,
    pub hmac: String,
}

/// 已信任的 SSH Host Key 记录。`endpoint` 形如 `host:port`，作为主键。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyRecord {
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    pub algorithm: Option<String>,
    pub fingerprint: String,
    pub trusted_at: UnixMillis,
    pub trusted_by: Option<String>,
}

/// 待写入的审计记录，字段与 `umt_core::AuditRecord` 一致，仅缺少自增主键。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewAuditEntry {
    pub session_id: Option<SessionId>,
    pub token_id: Option<TokenId>,
    pub connection_id: Option<ConnectionId>,
    pub kind: AuditKind,
    pub outcome: AuditOutcome,
    pub command: Option<String>,
    pub working_dir: Option<String>,
    pub started_at: UnixMillis,
    pub duration_ms: Option<u64>,
    pub state: ExecutionState,
    pub exit_code: Option<i32>,
    pub stdout_excerpt: Option<String>,
    pub stderr_excerpt: Option<String>,
    pub error: Option<String>,
    pub redacted: bool,
}

/// 连接列表筛选条件。`protocol_id` / `enabled` 精确匹配，
/// `search` 对 name、description、notes、tags 做大小写不敏感的包含匹配。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionFilter {
    pub protocol_id: Option<String>,
    pub enabled: Option<bool>,
    pub search: Option<String>,
    pub page: PageRequest,
}

/// 会话历史筛选条件。所有条件为 AND 关系，时间范围按 `started_at` 闭区间。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFilter {
    pub token_id: Option<TokenId>,
    pub connection_id: Option<ConnectionId>,
    pub state: Option<SessionState>,
    pub from: Option<UnixMillis>,
    pub to: Option<UnixMillis>,
    pub page: PageRequest,
}

/// 授权视图：连接本体 + 该 Token 是否被允许访问。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantView {
    pub connection: Connection,
    pub allowed: bool,
}
