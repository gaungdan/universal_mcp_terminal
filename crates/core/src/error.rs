use crate::capability::Capability;
use thiserror::Error;

/// 全项目统一错误类型。
///
/// 约束：
/// - 错误信息里**不得**出现密码、私钥、完整 Token。
/// - 每个错误都有稳定的 `code()`，供前端与 MCP 客户端判断类型。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UmtError {
    #[error("未找到{entity}：{id}")]
    NotFound { entity: &'static str, id: String },

    #[error("操作冲突：{0}")]
    Conflict(String),

    #[error("参数校验失败：{0}")]
    Validation(String),

    #[error("访问被拒绝：{0}")]
    AccessDenied(String),

    #[error("协议不支持能力：{}", .0.as_str())]
    UnsupportedCapability(Capability),

    #[error("数据存储错误：{0}")]
    Storage(String),

    #[error("操作超时：{0}")]
    Timeout(String),

    #[error("外部依赖错误：{0}")]
    External(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

impl UmtError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Validation(_) => "validation",
            Self::AccessDenied(_) => "access_denied",
            Self::UnsupportedCapability(_) => "unsupported_capability",
            Self::Storage(_) => "storage",
            Self::Timeout(_) => "timeout",
            Self::External(_) => "external",
            Self::Internal(_) => "internal",
        }
    }

    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code().to_string(),
            message: self.to_string(),
        }
    }
}

/// IPC / MCP 返回给调用方的错误结构。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl From<UmtError> for ErrorPayload {
    fn from(value: UmtError) -> Self {
        value.payload()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        let err = UmtError::AccessDenied("token 未授权该连接".into());
        assert_eq!(err.code(), "access_denied");
        assert_eq!(err.payload().code, "access_denied");
    }

    #[test]
    fn unsupported_capability_mentions_capability() {
        let err = UmtError::UnsupportedCapability(Capability::PortForward);
        assert!(err.to_string().contains("port_forward"));
    }
}
