use thiserror::Error;
use umt_core::{Capability, UmtError};

/// 协议层错误。分类必须足够细，前端与 MCP 可据此给出排错建议。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    #[error("配置无效（{field}）：{message}")]
    ConfigInvalid { field: String, message: String },

    #[error("认证失败：{0}")]
    AuthFailed(String),

    #[error("未知的 Host Key，指纹：{fingerprint}")]
    HostKeyUnknown { fingerprint: String },

    #[error("Host Key 不匹配，已保存：{expected}，本次：{actual}")]
    HostKeyMismatch { expected: String, actual: String },

    #[error("连接超时：{0}")]
    Timeout(String),

    #[error("网络错误：{0}")]
    Network(String),

    #[error("连接已断开：{0}")]
    Disconnected(String),

    #[error("协议不支持能力：{}", .0.as_str())]
    UnsupportedCapability(Capability),

    #[error("本地 I/O 错误：{0}")]
    Io(String),

    #[error("协议内部错误：{0}")]
    Internal(String),
}

impl From<ProviderError> for UmtError {
    fn from(value: ProviderError) -> Self {
        match value {
            ProviderError::ConfigInvalid { field, message } => {
                UmtError::Validation(format!("{field}: {message}"))
            }
            ProviderError::AuthFailed(msg) => UmtError::AccessDenied(msg),
            ProviderError::HostKeyUnknown { fingerprint } => {
                UmtError::AccessDenied(format!("未知 Host Key：{fingerprint}"))
            }
            ProviderError::HostKeyMismatch { expected, actual } => UmtError::AccessDenied(format!(
                "Host Key 不匹配（已保存 {expected}，本次 {actual}）"
            )),
            ProviderError::Timeout(msg) => UmtError::Timeout(msg),
            ProviderError::Network(msg)
            | ProviderError::Disconnected(msg)
            | ProviderError::Io(msg)
            | ProviderError::Internal(msg) => UmtError::External(msg),
            ProviderError::UnsupportedCapability(cap) => UmtError::UnsupportedCapability(cap),
        }
    }
}

/// 会话读写错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionError {
    #[error("会话已关闭")]
    Closed,

    #[error("会话操作超时：{0}")]
    Timeout(String),

    #[error("会话不支持能力：{}", .0.as_str())]
    UnsupportedCapability(Capability),

    #[error("会话 I/O 错误：{0}")]
    Io(String),

    #[error("会话内部错误：{0}")]
    Internal(String),
}

impl From<ProviderError> for SessionError {
    fn from(value: ProviderError) -> Self {
        match value {
            ProviderError::UnsupportedCapability(cap) => Self::UnsupportedCapability(cap),
            ProviderError::Timeout(msg) => Self::Timeout(msg),
            ProviderError::Io(msg)
            | ProviderError::Network(msg)
            | ProviderError::Disconnected(msg) => Self::Io(msg),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<SessionError> for UmtError {
    fn from(value: SessionError) -> Self {
        match value {
            SessionError::Closed => UmtError::Conflict("会话已关闭".into()),
            SessionError::Timeout(msg) => UmtError::Timeout(msg),
            SessionError::UnsupportedCapability(cap) => UmtError::UnsupportedCapability(cap),
            SessionError::Io(msg) | SessionError::Internal(msg) => UmtError::External(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_maps_to_validation() {
        let err = ProviderError::ConfigInvalid {
            field: "port".into(),
            message: "必须在 1-65535".into(),
        };
        assert_eq!(UmtError::from(err).code(), "validation");
    }

    #[test]
    fn host_key_problem_is_access_denied() {
        let err = ProviderError::HostKeyUnknown {
            fingerprint: "SHA256:abc".into(),
        };
        assert_eq!(UmtError::from(err).code(), "access_denied");
    }
}
