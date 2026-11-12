//! 领域模型：强类型 ID、连接聚合、能力声明、统一错误、应用事件。
//!
//! 约束：本 crate 不依赖 tokio、网络、数据库，只允许 serde / schemars / thiserror / uuid / time。
//! 契约冻结后由 T1 维护，其他工单只读。

#![deny(unsafe_code)]
