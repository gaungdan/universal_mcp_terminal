//! MCP Server：Streamable HTTP（rmcp）、Bearer 鉴权、工具注册与能力校验。
//!
//! 独占写入范围：`crates/mcp-server/**`。
//!
//! 设计要点：
//! - 每个工具调用都从 HTTP Header 解析 `Authorization: Bearer <token>` 并走
//!   `AccessControl::authenticate`；token 绝不来自工具参数。
//! - 涉及具体连接的调用必须先 `authorize_connection`，涉及会话的调用由
//!   `SessionManager` 校验归属；权限判断全部在后端完成。
//! - 不支持的能力直接返回 `UnsupportedCapability`，不伪造成功。
//! - 日志只记录错误码与脱敏后的错误文本，不记录 Token、密码、私钥或密文。

#![deny(unsafe_code)]

mod deps;
mod handler;
mod service;
mod tools;

#[cfg(test)]
mod testing;

pub use deps::{ConnectionConfigSource, McpDeps};
pub use handler::{UmtMcpServer, parse_bearer};
pub use service::{McpServer, McpServerConfig, McpStatus};
pub use tools::{TOOL_NAMES, tool_definitions};
