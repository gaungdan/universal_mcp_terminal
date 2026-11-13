use crate::ids::TokenId;
use serde::{Deserialize, Serialize};

/// 一次已认证请求的调用方上下文。
///
/// 由 Access Control 在每次 MCP 请求时构造；所有后续权限判断都以它为准，
/// 禁止从请求参数中读取 token_id 或 connection 授权信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenContext {
    pub token_id: TokenId,
    pub token_name: String,
    /// AI 客户端标识，可来源于 User-Agent 或 MCP initialize 参数。
    pub client_info: Option<String>,
}

impl TokenContext {
    pub fn new(token_id: TokenId, token_name: impl Into<String>) -> Self {
        Self {
            token_id,
            token_name: token_name.into(),
            client_info: None,
        }
    }

    pub fn with_client_info(mut self, client_info: Option<String>) -> Self {
        self.client_info = client_info;
        self
    }
}
