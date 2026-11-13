use thiserror::Error;
use umt_core::UmtError;

/// 安全层错误。错误信息中不得包含密钥、明文凭据或完整 Token。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SecurityError {
    #[error("加密错误：{0}")]
    Crypto(String),

    #[error("系统凭据库不可用：{0}")]
    KeyStore(String),

    #[error("密钥格式无效")]
    InvalidKey,

    #[error("凭据已损坏或被篡改")]
    CorruptedSecret,

    #[error("Token 格式无效")]
    InvalidToken,
}

impl From<SecurityError> for UmtError {
    fn from(value: SecurityError) -> Self {
        match value {
            SecurityError::Crypto(msg) => UmtError::Storage(format!("加密错误：{msg}")),
            SecurityError::CorruptedSecret => UmtError::Storage("凭据已损坏或被篡改".into()),
            SecurityError::KeyStore(msg) => UmtError::External(msg),
            SecurityError::InvalidKey => UmtError::Validation("密钥格式无效".into()),
            SecurityError::InvalidToken => UmtError::Validation("Token 格式无效".into()),
        }
    }
}
