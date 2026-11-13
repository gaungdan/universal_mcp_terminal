//! IPC 契约：Tauri 命令与前后端共用的 DTO。
//!
//! 契约状态：T5 冻结。Rust 侧字段命名统一 `camelCase`，
//! 前端类型定义必须与 `dto.rs` 逐字段对齐（见 `apps/desktop/src/api/types.ts`）。

pub mod commands;
pub mod dto;

pub use dto::*;
