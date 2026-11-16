//! 统一会话管理：会话状态机、Token/Connection 绑定、活动会话回收。
//!
//! 约束：本 crate 不依赖任何具体协议；Provider 一律从注册中心按 ID 取得。

#![deny(unsafe_code)]

pub mod manager;
pub mod policy;
pub mod shell;

pub use manager::{OpenSessionRequest, SessionLimits, SessionManager, SessionPurpose};
pub use policy::{CommandGuard, ExecutionMode, PolicyDecision};
