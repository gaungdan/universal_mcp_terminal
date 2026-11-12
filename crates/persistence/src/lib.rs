//! 持久化：SQLite 连接池、迁移执行、仓库 trait 与实现、启动恢复。
//!
//! 契约（建表 SQL / 仓库 trait）由 T3 冻结；具体实现由 T6 填充。
//! 独占写入范围：`crates/persistence/**`，其中 `migrations/**` 只允许 T6 写入。

#![deny(unsafe_code)]
