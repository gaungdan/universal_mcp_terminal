//! 协议 Provider 统一抽象。
//!
//! 本 crate 只描述"协议能做什么"，不包含任何具体协议的实现。
//! 契约状态：T2 冻结。

#![deny(unsafe_code)]

pub mod error;
pub mod mock;
pub mod provider;
pub mod schema;
pub mod session;
pub mod test_result;

pub use error::{ProviderError, SessionError};
pub use mock::MockProvider;
pub use provider::{ProtocolDescriptor, ProtocolProvider};
pub use schema::{
    ConfigField, ConfigSchema, FieldCondition, FieldKind, FieldValidation, SelectOption,
};
pub use session::{
    ConnectOptions, ExecRequest, ExecResult, FileChunk, FileReadRequest, OutputChunk, SecretValues,
    TerminalSession,
};
pub use test_result::{ConnectionTestResult, HostKeyInfo, TestStep, TestStepStatus};
