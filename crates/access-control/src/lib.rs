//! 访问控制：Token 生命周期、Token↔Connection 多对多授权、每请求七项校验、撤销联动。
//!
//! 独占写入范围：`crates/access-control/**`。

#![deny(unsafe_code)]
