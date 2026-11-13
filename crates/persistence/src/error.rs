//! 持久化层错误。CONTRACT-FROZEN: T3

use thiserror::Error;
use umt_core::UmtError;

/// 存储层统一错误。
///
/// 约束：错误信息中**不得**出现密码、私钥、完整 Token 或密文内容。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PersistenceError {
    /// 目标记录不存在。
    #[error("未找到{entity}：{id}")]
    NotFound { entity: &'static str, id: String },

    /// 乐观锁版本不符或状态冲突。
    #[error("操作冲突：{0}")]
    Conflict(String),

    /// 唯一约束冲突（如连接名、Token 前缀重复）。
    #[error("唯一约束冲突：{0}")]
    UniqueViolation(String),

    /// 调用方传入的数据不合法（如 JSON 无法解析）。
    #[error("数据不合法：{0}")]
    InvalidData(String),

    /// 底层存储错误（SQLite / IO / 迁移等）。
    #[error("存储后端错误：{0}")]
    Backend(String),
}

impl PersistenceError {
    pub fn not_found(entity: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity,
            id: id.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn unique_violation(message: impl Into<String>) -> Self {
        Self::UniqueViolation(message.into())
    }

    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }

    /// 把任意可展示的底层错误折叠为 `Backend`。
    pub fn backend(err: impl std::fmt::Display) -> Self {
        Self::Backend(err.to_string())
    }
}

impl From<PersistenceError> for UmtError {
    fn from(value: PersistenceError) -> Self {
        match value {
            PersistenceError::NotFound { entity, id } => Self::NotFound { entity, id },
            PersistenceError::Conflict(message) => Self::Conflict(message),
            // 唯一冲突对调用方而言同样是「资源已存在」的冲突
            PersistenceError::UniqueViolation(message) => {
                Self::Conflict(format!("唯一约束冲突：{message}"))
            }
            PersistenceError::InvalidData(message) => Self::Validation(message),
            PersistenceError::Backend(message) => Self::Storage(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_maps_to_umt_error_codes() {
        let err = UmtError::from(PersistenceError::not_found("连接", "abc"));
        assert_eq!(err.code(), "not_found");

        let err = UmtError::from(PersistenceError::Conflict("revision 不符".into()));
        assert_eq!(err.code(), "conflict");

        let err = UmtError::from(PersistenceError::unique_violation("连接名重复"));
        assert_eq!(err.code(), "conflict");

        let err = UmtError::from(PersistenceError::invalid_data("字段缺失"));
        assert_eq!(err.code(), "validation");

        let err = UmtError::from(PersistenceError::backend("disk I/O"));
        assert_eq!(err.code(), "storage");
    }

    #[test]
    fn not_found_display_contains_entity_and_id() {
        let err = PersistenceError::not_found("会话", "s-1");
        assert!(err.to_string().contains("会话"));
        assert!(err.to_string().contains("s-1"));
    }
}
