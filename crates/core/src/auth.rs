use crate::ids::TokenId;
use crate::token::TokenPermission;
use serde::{Deserialize, Serialize};

/// 一次已认证请求的调用方上下文。
///
/// 由 Access Control 在每次 MCP 请求时构造；所有后续权限判断都以它为准，
/// 禁止从请求参数中读取 token_id 或 connection 授权信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenContext {
    pub token_id: TokenId,
    pub token_name: String,
    /// Token 权限等级，鉴权时从数据库读取，禁止由请求参数构造。
    #[serde(default)]
    pub permission: TokenPermission,
    /// 是否允许交互式终端。
    #[serde(default)]
    pub allow_interactive: bool,
    /// AI 客户端标识，可来源于 User-Agent 或 MCP initialize 参数。
    pub client_info: Option<String>,
}

impl TokenContext {
    pub fn new(token_id: TokenId, token_name: impl Into<String>) -> Self {
        Self {
            token_id,
            token_name: token_name.into(),
            permission: TokenPermission::ReadOnly,
            allow_interactive: false,
            client_info: None,
        }
    }

    pub fn with_permission(mut self, permission: TokenPermission) -> Self {
        self.permission = permission;
        self
    }

    pub fn with_interactive(mut self, allow_interactive: bool) -> Self {
        self.allow_interactive = allow_interactive;
        self
    }

    pub fn with_client_info(mut self, client_info: Option<String>) -> Self {
        self.client_info = client_info;
        self
    }
}
