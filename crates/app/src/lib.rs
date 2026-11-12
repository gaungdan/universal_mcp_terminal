//! 应用编排层：启动初始化、托管状态、Tauri 命令与 IPC DTO。
//!
//! 独占写入范围：`crates/app/**`（`src/ipc/**` 为契约，冻结后只读）。

#![deny(unsafe_code)]
