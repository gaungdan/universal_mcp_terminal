use crate::ids::TokenId;
use crate::time::UnixMillis;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Token 权限等级。
///
/// `ReadOnly` 允许只读查询类命令，`WriteAccess` 允许普通写操作；
/// 两者都禁止提权命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TokenPermission {
    #[default]
    ReadOnly,
    WriteAccess,
}

impl TokenPermission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WriteAccess => "write_access",
        }
    }
}

impl FromStr for TokenPermission {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "read_only" | "ReadOnly" => Ok(Self::ReadOnly),
            "write_access" | "WriteAccess" => Ok(Self::WriteAccess),
            other => Err(format!(
                "未知 Token 权限：{other}（可选 read_only / write_access）"
            )),
        }
    }
}

/// Token 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStatus {
    Active,
    Disabled,
    Revoked,
}

impl TokenStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Revoked => "revoked",
        }
    }

    pub fn is_usable(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Token 的对外可见信息。
///
/// 安全约束：本结构体**不包含** Token 明文，也不包含校验用的 HMAC 值，
/// 因此可以安全地通过 IPC 返回给前端或记录到日志。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub id: TokenId,
    pub name: String,
    pub description: Option<String>,
    /// 明文前缀（前 8 位），仅用于列表展示与用户识别。
    pub prefix: String,
    pub status: TokenStatus,
    #[serde(default)]
    pub permission: TokenPermission,
    #[serde(default)]
    pub allow_interactive: bool,
    pub expires_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
    pub last_used_at: Option<UnixMillis>,
}

impl Token {
    /// 是否在某一时刻可用：状态为 active 且未过期。
    pub fn is_valid_at(&self, now: UnixMillis) -> bool {
        if !self.status.is_usable() {
            return false;
        }
        match self.expires_at {
            Some(expires_at) => expires_at > now,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(status: TokenStatus, expires_at: Option<UnixMillis>) -> Token {
        Token {
            id: TokenId::new(),
            name: "claude".into(),
            description: None,
            prefix: "umt_abcd".into(),
            status,
            permission: TokenPermission::ReadOnly,
            allow_interactive: false,
            expires_at,
            created_at: UnixMillis::now(),
            last_used_at: None,
        }
    }

    #[test]
    fn expired_token_is_invalid() {
        let now = UnixMillis::now();
        let expired = token(TokenStatus::Active, Some(UnixMillis(now.0 - 1)));
        assert!(!expired.is_valid_at(now));
    }

    #[test]
    fn revoked_token_is_invalid() {
        let now = UnixMillis::now();
        assert!(!token(TokenStatus::Revoked, None).is_valid_at(now));
        assert!(token(TokenStatus::Active, None).is_valid_at(now));
    }
}
