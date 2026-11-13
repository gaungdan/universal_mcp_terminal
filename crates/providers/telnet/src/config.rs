//! Telnet 连接的配置解析与正则编译。

use std::fmt;
use std::time::Duration;

use regex::Regex;
use umt_core::ConnectionConfig;
use umt_protocol_api::ProviderError;
use umt_protocol_api::session::SecretValues;

pub const DEFAULT_PORT: u16 = 23;
pub const DEFAULT_TIMEOUT_SECS: u64 = 15;
pub const DEFAULT_LOGIN_PROMPT: &str = "(login:|Username:|username:)";
pub const DEFAULT_PASSWORD_PROMPT: &str = "(Password:|password:)";
pub const DEFAULT_COMMAND_PROMPT: &str = "[#$>] ?$";
pub const DEFAULT_TERMINAL_TYPE: &str = "xterm";

/// 解析后的 Telnet 配置。
///
/// `password` 来自 `ConnectOptions.secrets`，绝不从连接配置 JSON 中读取，也绝不打印。
pub(crate) struct TelnetConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub connection_timeout: Duration,
    pub login_prompt: Regex,
    pub password_prompt: Regex,
    pub command_prompt: Regex,
    pub terminal_type: String,
}

impl fmt::Debug for TelnetConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelnetConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("connection_timeout", &self.connection_timeout)
            .field("login_prompt", &self.login_prompt.as_str())
            .field("password_prompt", &self.password_prompt.as_str())
            .field("command_prompt", &self.command_prompt.as_str())
            .field("terminal_type", &self.terminal_type)
            .finish()
    }
}

impl TelnetConfig {
    pub(crate) fn from_config(
        config: &ConnectionConfig,
        secrets: &SecretValues,
    ) -> Result<Self, ProviderError> {
        let host = config
            .get_str("host")
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .ok_or_else(|| ProviderError::ConfigInvalid {
                field: "host".into(),
                message: "主机不能为空".into(),
            })?
            .to_string();

        let port = match config.values.get("port").filter(|value| !value.is_null()) {
            Some(value) => {
                let port = value.as_u64().ok_or_else(|| ProviderError::ConfigInvalid {
                    field: "port".into(),
                    message: "端口必须是整数".into(),
                })?;
                let port = u16::try_from(port).map_err(|_| ProviderError::ConfigInvalid {
                    field: "port".into(),
                    message: "端口必须在 1..65535 之间".into(),
                })?;
                if port == 0 {
                    return Err(ProviderError::ConfigInvalid {
                        field: "port".into(),
                        message: "端口必须在 1..65535 之间".into(),
                    });
                }
                port
            }
            None => DEFAULT_PORT,
        };

        let username = config
            .get_str("username")
            .map(str::trim)
            .filter(|username| !username.is_empty())
            .map(str::to_string);

        // 只读取机密值，忽略配置里的 password 字段。
        let password = secrets
            .get("password")
            .filter(|password| !password.is_empty())
            .map(str::to_string);

        let timeout_secs = config
            .values
            .get("connection_timeout_secs")
            .and_then(|value| value.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, 120);

        let terminal_type = config
            .get_str("terminal_type")
            .map(str::trim)
            .filter(|terminal_type| !terminal_type.is_empty())
            .unwrap_or(DEFAULT_TERMINAL_TYPE)
            .to_string();

        Ok(Self {
            host,
            port,
            username,
            password,
            connection_timeout: Duration::from_secs(timeout_secs),
            login_prompt: compile_pattern(config, "login_prompt", DEFAULT_LOGIN_PROMPT)?,
            password_prompt: compile_pattern(config, "password_prompt", DEFAULT_PASSWORD_PROMPT)?,
            command_prompt: compile_pattern(config, "command_prompt", DEFAULT_COMMAND_PROMPT)?,
            terminal_type,
        })
    }

    /// 校验用户填写的提示符正则可编译，供 Provider 的 `validate_config` 使用。
    pub(crate) fn validate_patterns(config: &ConnectionConfig) -> Result<(), ProviderError> {
        for (key, default) in [
            ("login_prompt", DEFAULT_LOGIN_PROMPT),
            ("password_prompt", DEFAULT_PASSWORD_PROMPT),
            ("command_prompt", DEFAULT_COMMAND_PROMPT),
        ] {
            compile_pattern(config, key, default)?;
        }
        Ok(())
    }
}

fn compile_pattern(
    config: &ConnectionConfig,
    key: &str,
    default: &str,
) -> Result<Regex, ProviderError> {
    let pattern = config
        .get_str(key)
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .unwrap_or(default);
    Regex::new(pattern).map_err(|err| ProviderError::ConfigInvalid {
        field: key.to_string(),
        message: format!("不是合法的正则表达式：{err}"),
    })
}
