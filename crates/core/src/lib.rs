//! Universal MCP Terminal 领域模型。
//!
//! 本 crate 是全部业务模块的公共底座，只允许 serde / serde_json / thiserror / uuid / time。
//! 禁止依赖 tokio、网络、数据库与任何具体协议实现。
//!
//! 契约状态：T1 冻结。其他工单如需扩展字段，必须走主控的契约变更流程。

#![deny(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod capability;
pub mod connection;
pub mod error;
pub mod events;
pub mod ids;
pub mod pagination;
pub mod session;
pub mod time;
pub mod token;

pub use audit::{AuditKind, AuditOutcome, AuditQuery, AuditRecord};
pub use auth::TokenContext;
pub use capability::{Capability, CapabilitySet};
pub use connection::{Connection, ConnectionConfig};
pub use error::{ErrorPayload, UmtError};
pub use events::AppEvent;
pub use ids::{AuditId, ConnectionId, SessionId, TokenId};
pub use pagination::{Page, PageRequest};
pub use session::{ExecutionState, SessionInfo, SessionState};
pub use time::UnixMillis;
pub use token::{Token, TokenStatus};
