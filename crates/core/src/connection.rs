use crate::ids::ConnectionId;
use crate::time::UnixMillis;
use serde::{Deserialize, Serialize};

/// 连接聚合。只包含与协议无关的通用字段。
///
/// 协议专属字段全部放在 `ConnectionConfig.values` 中，由对应 Provider 的 ConfigSchema 定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub name: String,
    /// 协议 ID，对应 Provider 的 `protocol_id`，例如 `ssh` / `telnet` / `wsl`。
    pub protocol_id: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
    /// 乐观锁版本号，每次成功更新 +1。
    pub revision: u32,
}

impl Connection {
    pub fn is_available(&self) -> bool {
        self.enabled
    }
}

/// 协议配置值。`values` 必须是 JSON 对象，且只允许出现 Provider Schema 声明的字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub protocol_id: String,
    pub values: serde_json::Value,
}

impl ConnectionConfig {
    pub fn new(protocol_id: impl Into<String>, values: serde_json::Value) -> Self {
        Self {
            protocol_id: protocol_id.into(),
            values,
        }
    }

    /// 校验基础结构：必须是对象。字段级校验由 Provider 负责。
    pub fn validate_shape(&self) -> Result<(), String> {
        if !self.values.is_object() {
            return Err("连接配置必须是 JSON 对象".to_string());
        }
        if self.protocol_id.trim().is_empty() {
            return Err("协议 ID 不能为空".to_string());
        }
        Ok(())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(|v| v.as_str())
    }

    pub fn get_u16(&self, key: &str) -> Option<u16> {
        self.values
            .get(key)
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(|v| v.as_bool())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_shape_requires_object() {
        let cfg = ConnectionConfig::new("ssh", serde_json::json!("not-an-object"));
        assert!(cfg.validate_shape().is_err());

        let cfg = ConnectionConfig::new("ssh", serde_json::json!({ "host": "127.0.0.1" }));
        assert!(cfg.validate_shape().is_ok());
        assert_eq!(cfg.get_str("host"), Some("127.0.0.1"));
    }

    #[test]
    fn disabled_connection_is_not_available() {
        let connection = Connection {
            id: ConnectionId::new(),
            name: "demo".into(),
            protocol_id: "ssh".into(),
            description: None,
            tags: vec![],
            notes: None,
            enabled: false,
            created_at: UnixMillis::now(),
            updated_at: UnixMillis::now(),
            revision: 1,
        };
        assert!(!connection.is_available());
    }
}
