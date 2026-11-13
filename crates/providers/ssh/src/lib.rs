//! SSH Provider（russh）：密码/私钥认证、Host Key 校验、keepalive、exec、交互 shell、resize。
//!
//! 独占写入范围：`crates/providers/ssh/**`。

#![deny(unsafe_code)]

mod config;
mod error;
mod host_key;
mod provider;
mod session;

pub use provider::SshProvider;
pub use session::SshSession;
