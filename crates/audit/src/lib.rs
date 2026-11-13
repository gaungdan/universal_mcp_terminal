//! 审计：敏感信息脱敏、记录追加、条件查询与导出。
//!
//! 约束：审计只追加，不修改、不删除；任何写库前必须先经过 `Redactor`。

#![deny(unsafe_code)]

pub mod redact;
pub mod service;

pub use redact::{RedactionOutcome, Redactor};
pub use service::AuditService;
