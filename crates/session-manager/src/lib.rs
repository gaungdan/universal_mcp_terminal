//! 统一会话管理：会话状态机、Token/Connection 绑定、输出缓冲与增量游标、超时回收。
//!
//! 独占写入范围：`crates/session-manager/**`。

#![deny(unsafe_code)]
