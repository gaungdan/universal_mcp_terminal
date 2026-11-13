use crate::ids::{AuditId, ConnectionId, SessionId, TokenId};
use crate::session::ExecutionState;
use crate::time::UnixMillis;
use serde::{Deserialize, Serialize};

/// 审计事件类型。新增类型只能追加，不能修改已有语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    SessionOpened,
    SessionClosed,
    CommandExecuted,
    InputSent,
    OutputRead,
    PermissionDenied,
    ConnectionCreated,
    ConnectionUpdated,
    ConnectionDeleted,
    ConnectionTested,
    TokenCreated,
    TokenDisabled,
    TokenRevoked,
    GrantChanged,
    McpServerStarted,
    McpServerStopped,
}

/// 审计结果。`Denied` 表示权限拒绝，`Failed` 表示放行后执行失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Failed,
}

/// 一条审计记录。敏感字段必须在此结构被写入之前完成脱敏。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: AuditId,
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
    /// 该记录是否发生过脱敏处理。
    pub redacted: bool,
}

/// 审计查询条件。所有条件为 AND 关系，时间范围为闭区间。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQuery {
    pub token_id: Option<TokenId>,
    pub connection_id: Option<ConnectionId>,
    pub session_id: Option<SessionId>,
    pub kind: Option<AuditKind>,
    pub outcome: Option<AuditOutcome>,
    pub from: Option<UnixMillis>,
    pub to: Option<UnixMillis>,
    pub offset: u32,
    pub limit: u32,
}

impl AuditQuery {
    pub fn limit_or_default(&self) -> u32 {
        if self.limit == 0 {
            100
        } else {
            self.limit.min(500)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_limit_is_bounded() {
        let mut query = AuditQuery::default();
        assert_eq!(query.limit_or_default(), 100);
        query.limit = 10_000;
        assert_eq!(query.limit_or_default(), 500);
    }
}
