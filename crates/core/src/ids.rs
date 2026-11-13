use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

uuid_id!(
    /// 连接（Connection）唯一标识。
    ConnectionId
);
uuid_id!(
    /// 访问 Token 唯一标识。
    TokenId
);
uuid_id!(
    /// 终端会话唯一标识。
    SessionId
);

/// 审计记录标识，对应 SQLite 自增主键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuditId(pub i64);

impl fmt::Display for AuditId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_id_roundtrips_through_string() {
        let id = ConnectionId::new();
        let parsed = ConnectionId::from_str(&id.to_string()).expect("应能解析自身字符串");
        assert_eq!(id, parsed);
    }

    #[test]
    fn ids_are_stable_across_serde() {
        let id = TokenId::new();
        let json = serde_json::to_string(&id).expect("序列化失败");
        assert_eq!(json, format!("\"{id}\""));
        let back: TokenId = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(id, back);
    }
}
