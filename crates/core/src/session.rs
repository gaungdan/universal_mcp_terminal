use crate::ids::{ConnectionId, SessionId, TokenId};
use crate::time::UnixMillis;
use serde::{Deserialize, Serialize};

/// 会话状态机：`Connecting -> Connected -> Running -> Closed`，异常进入 `Failed` / `Interrupted`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Connecting,
    Connected,
    Running,
    Closed,
    Failed,
    Interrupted,
}

impl SessionState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Connecting | Self::Connected | Self::Running)
    }

    /// 状态迁移是否合法。非法迁移必须由 Session Manager 拒绝，而不是静默覆盖。
    pub fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Connecting => matches!(next, Self::Connected | Self::Failed | Self::Interrupted),
            Self::Connected => matches!(
                next,
                Self::Running | Self::Closed | Self::Failed | Self::Interrupted
            ),
            Self::Running => matches!(next, Self::Closed | Self::Failed | Self::Interrupted),
            Self::Closed | Self::Failed | Self::Interrupted => false,
        }
    }
}

/// 会话信息。必须同时绑定 Token 与 Connection。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub token_id: TokenId,
    pub connection_id: ConnectionId,
    pub protocol_id: String,
    /// AI 客户端标识，可从 MCP 请求头或初始化参数中获取，获取不到时为 None。
    pub client_info: Option<String>,
    pub state: SessionState,
    pub started_at: UnixMillis,
    pub ended_at: Option<UnixMillis>,
    pub close_reason: Option<String>,
    pub rows: u16,
    pub cols: u16,
}

/// 命令执行状态。
///
/// 注意：网络断开不等于命令失败——远端可能已经执行，因此必须区分 `Unknown`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    /// 已发送，尚未确认远端开始执行。
    Sent,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    /// 结果未知（连接中断、无响应等）。
    Unknown,
}

impl ExecutionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Unknown
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_session_cannot_transition_again() {
        assert!(!SessionState::Closed.can_transition_to(SessionState::Running));
    }

    #[test]
    fn running_session_can_close() {
        assert!(SessionState::Running.can_transition_to(SessionState::Closed));
        assert!(SessionState::Connecting.can_transition_to(SessionState::Connected));
        assert!(!SessionState::Connecting.can_transition_to(SessionState::Running));
    }

    #[test]
    fn unknown_execution_state_is_terminal() {
        assert!(ExecutionState::Unknown.is_terminal());
        assert!(!ExecutionState::Running.is_terminal());
    }
}
