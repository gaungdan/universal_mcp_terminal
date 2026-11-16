//! MCP 工具级权限门。
//!
//! 所有工具调用都先经过这里，再进入连接授权与命令策略。
//! 交互式终端属于高风险能力，必须同时满足 WriteAccess 与显式开关。

use umt_core::{TokenContext, TokenPermission, UmtError};

pub struct PermissionGate;

impl PermissionGate {
    /// 检查当前 Token 是否允许调用指定 MCP 工具。
    pub fn authorize_tool(token: &TokenContext, tool: &str) -> Result<(), UmtError> {
        match tool {
            "open_session" | "send_input" => Self::authorize_interactive(token),
            _ => Ok(()),
        }
    }

    /// 交互式终端要求 WriteAccess，且 Token 显式开启 `allow_interactive`。
    pub fn authorize_interactive(token: &TokenContext) -> Result<(), UmtError> {
        if token.permission != TokenPermission::WriteAccess {
            return Err(UmtError::AccessDenied(
                "ReadOnly Token 不允许使用交互式终端".into(),
            ));
        }
        if !token.allow_interactive {
            return Err(UmtError::AccessDenied(
                "该 Token 未开启交互式终端权限".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_core::{TokenId, TokenPermission};

    fn token(permission: TokenPermission, allow_interactive: bool) -> TokenContext {
        TokenContext::new(TokenId::new(), "test")
            .with_permission(permission)
            .with_interactive(allow_interactive)
    }

    #[test]
    fn read_only_cannot_open_interactive() {
        let context = token(TokenPermission::ReadOnly, true);
        assert!(PermissionGate::authorize_tool(&context, "open_session").is_err());
        assert!(PermissionGate::authorize_tool(&context, "send_input").is_err());
    }

    #[test]
    fn write_access_requires_interactive_switch() {
        let disabled = token(TokenPermission::WriteAccess, false);
        assert!(PermissionGate::authorize_tool(&disabled, "open_session").is_err());

        let enabled = token(TokenPermission::WriteAccess, true);
        assert!(PermissionGate::authorize_tool(&enabled, "open_session").is_ok());
        assert!(PermissionGate::authorize_tool(&enabled, "send_input").is_ok());
    }

    #[test]
    fn read_only_query_tools_are_allowed() {
        let context = token(TokenPermission::ReadOnly, false);
        for tool in [
            "list_connections",
            "get_connection_info",
            "test_connection",
            "execute_command",
            "read_output",
            "resize_terminal",
            "close_session",
        ] {
            assert!(
                PermissionGate::authorize_tool(&context, tool).is_ok(),
                "{tool}"
            );
        }
    }
}
