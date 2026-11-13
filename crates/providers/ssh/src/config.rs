//! SSH 配置解析与认证凭据解析。
//!
//! `ConnectionConfig.values` 只包含非敏感字段；密码、私钥与口令统一从
//! [`umt_protocol_api::ConnectOptions::secrets`] 读取，避免随配置落库或写日志。

use std::time::Duration;

use umt_core::ConnectionConfig;
use umt_protocol_api::ProviderError;
use zeroize::Zeroizing;

/// 认证方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthType {
    Password,
    PrivateKey,
}

/// 已解析并校验过的 SSH 连接配置（不含敏感信息）。
#[derive(Debug, Clone)]
pub(crate) struct SshConfig {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) auth_type: AuthType,
    pub(crate) connection_timeout: Duration,
    pub(crate) keepalive: Option<Duration>,
    pub(crate) terminal_type: String,
    pub(crate) default_working_dir: Option<String>,
}

impl SshConfig {
    /// 从通用配置对象中提取 SSH 字段，未提供的可选字段使用文档约定默认值。
    pub(crate) fn from_config(config: &ConnectionConfig) -> Result<Self, ProviderError> {
        let host = required_text(config, "host", "主机")?;
        let username = required_text(config, "username", "用户名")?;
        let auth_type = match config.get_str("auth_type").unwrap_or("password") {
            "password" => AuthType::Password,
            "private_key" => AuthType::PrivateKey,
            other => {
                return Err(ProviderError::ConfigInvalid {
                    field: "auth_type".into(),
                    message: format!("不支持的认证方式：{other}"),
                });
            }
        };

        let port = config.get_u16("port").unwrap_or(22);
        let timeout_secs = config
            .values
            .get("connection_timeout_secs")
            .and_then(|value| value.as_u64())
            .unwrap_or(15)
            .clamp(1, 120);
        let keepalive_secs = config
            .values
            .get("keepalive_secs")
            .and_then(|value| value.as_u64())
            .unwrap_or(30)
            .min(600);
        let terminal_type = config
            .get_str("terminal_type")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("xterm-256color")
            .to_string();
        let default_working_dir = config
            .get_str("default_working_dir")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        Ok(Self {
            host,
            port,
            username,
            auth_type,
            connection_timeout: Duration::from_secs(timeout_secs),
            keepalive: (keepalive_secs > 0).then(|| Duration::from_secs(keepalive_secs)),
            terminal_type,
            default_working_dir,
        })
    }
}

/// 认证输入。持有 [`Zeroizing`]，离开作用域时擦除明文。
pub(crate) enum SshAuth {
    Password(Zeroizing<String>),
    PrivateKey {
        pem: Zeroizing<String>,
        passphrase: Option<Zeroizing<String>>,
    },
}

/// 解析认证凭据。`lookup` 负责按字段名返回敏感值（正常连接来自加密存储）。
pub(crate) fn resolve_auth<'a>(
    config: &SshConfig,
    lookup: impl Fn(&str) -> Option<&'a str>,
) -> Result<SshAuth, ProviderError> {
    match config.auth_type {
        AuthType::Password => {
            let password = required_secret(&lookup, "password", "密码")?;
            Ok(SshAuth::Password(Zeroizing::new(password.to_string())))
        }
        AuthType::PrivateKey => {
            let pem = required_secret(&lookup, "private_key", "私钥")?;
            let passphrase = lookup("private_key_passphrase")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| Zeroizing::new(value.to_string()));
            Ok(SshAuth::PrivateKey {
                pem: Zeroizing::new(pem.to_string()),
                passphrase,
            })
        }
    }
}

/// 连接测试没有 `ConnectOptions`，只能从表单提交的配置对象中读取敏感字段。
pub(crate) fn resolve_auth_from_config(
    config: &SshConfig,
    values: &ConnectionConfig,
) -> Result<SshAuth, ProviderError> {
    resolve_auth(config, |key| values.get_str(key))
}

fn required_text(
    config: &ConnectionConfig,
    key: &str,
    label: &str,
) -> Result<String, ProviderError> {
    config
        .get_str(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ProviderError::ConfigInvalid {
            field: key.to_string(),
            message: format!("{label} 为必填项"),
        })
}

fn required_secret<'a>(
    lookup: &impl Fn(&str) -> Option<&'a str>,
    key: &str,
    label: &str,
) -> Result<&'a str, ProviderError> {
    lookup(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProviderError::ConfigInvalid {
            field: key.to_string(),
            message: format!("{label}认证需要提供{label}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection_config(json: &str) -> ConnectionConfig {
        let values = json.parse().expect("测试 JSON 非法");
        ConnectionConfig::new("ssh", values)
    }

    #[test]
    fn applies_documented_defaults() {
        let config = SshConfig::from_config(&connection_config(
            r#"{"host":"example.com","username":"root","auth_type":"password"}"#,
        ))
        .expect("配置应可解析");
        assert_eq!(config.port, 22);
        assert_eq!(config.connection_timeout, Duration::from_secs(15));
        assert_eq!(config.keepalive, Some(Duration::from_secs(30)));
        assert_eq!(config.terminal_type, "xterm-256color");
        assert!(config.default_working_dir.is_none());
    }

    #[test]
    fn keepalive_zero_disables_keepalive() {
        let config = SshConfig::from_config(&connection_config(
            r#"{"host":"h","port":2222,"username":"u","auth_type":"password","keepalive_secs":0}"#,
        ))
        .expect("配置应可解析");
        assert_eq!(config.port, 2222);
        assert_eq!(config.keepalive, None);
    }

    #[test]
    fn password_auth_requires_secret() {
        let config = SshConfig::from_config(&connection_config(
            r#"{"host":"h","username":"u","auth_type":"password"}"#,
        ))
        .expect("配置应可解析");
        let err = match resolve_auth(&config, |_| None) {
            Ok(_) => panic!("缺少密码时应失败"),
            Err(err) => err,
        };
        assert!(
            matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "password")
        );
    }

    #[test]
    fn password_auth_reads_secret_value() {
        let config = SshConfig::from_config(&connection_config(
            r#"{"host":"h","username":"u","auth_type":"password"}"#,
        ))
        .expect("配置应可解析");
        let auth = resolve_auth(&config, |key| (key == "password").then_some("s3cret"))
            .expect("应解析出密码");
        assert!(matches!(auth, SshAuth::Password(_)));
    }

    #[test]
    fn private_key_auth_requires_key_and_reads_passphrase() {
        let config = SshConfig::from_config(&connection_config(
            r#"{"host":"h","username":"u","auth_type":"private_key"}"#,
        ))
        .expect("配置应可解析");

        let err = match resolve_auth(&config, |_| None) {
            Ok(_) => panic!("缺少私钥时应失败"),
            Err(err) => err,
        };
        assert!(
            matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "private_key")
        );

        let auth = resolve_auth(&config, |key| match key {
            "private_key" => Some("-----BEGIN OPENSSH PRIVATE KEY-----"),
            "private_key_passphrase" => Some("pass"),
            _ => None,
        })
        .expect("应解析出私钥");
        match auth {
            SshAuth::PrivateKey { passphrase, .. } => {
                assert_eq!(passphrase.as_deref().map(String::as_str), Some("pass"));
            }
            SshAuth::Password(_) => panic!("认证方式解析错误"),
        }
    }
}
