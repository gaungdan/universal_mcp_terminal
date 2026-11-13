//! 访问控制：Token 生命周期、Token↔Connection 授权、每次调用的实时权限校验。
//!
//! 安全边界：所有结论都基于后端数据，前端展示不参与任何权限判断。

#![deny(unsafe_code)]

pub mod service;

pub use service::{AccessControl, CreatedToken, SessionRevoker};
