use crate::ids::{AuditId, ConnectionId, SessionId, TokenId};
use crate::session::SessionState;
use serde::{Deserialize, Serialize};

/// 后端推送给前端的应用事件（Tauri event）。
///
/// 事件名固定为 `umt://event`，负载使用本枚举，tag 为 `type`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    ConnectionChanged {
        connection_id: ConnectionId,
    },
    TokenChanged {
        token_id: TokenId,
    },
    GrantChanged {
        token_id: TokenId,
        connection_id: ConnectionId,
        allowed: bool,
    },
    SessionChanged {
        session_id: SessionId,
        state: SessionState,
    },
    McpServerStateChanged {
        running: bool,
        endpoint: Option<String>,
    },
    AuditAppended {
        audit_id: AuditId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_with_type_tag() {
        let event = AppEvent::McpServerStateChanged {
            running: true,
            endpoint: Some("http://127.0.0.1:47321/mcp".into()),
        };
        let json = serde_json::to_value(&event).expect("序列化失败");
        assert_eq!(json["type"], "mcp_server_state_changed");
        assert_eq!(json["running"], true);
    }
}
