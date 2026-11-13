//! Telnet Provider：配置 Schema、真实连接测试与会话创建。

use std::time::Instant;

use async_trait::async_trait;
use umt_core::{Capability, CapabilitySet, ConnectionConfig, SessionId};
use umt_protocol_api::session::{ConnectOptions, SecretValues, TerminalSession};
use umt_protocol_api::{
    ConfigField, ConfigSchema, ConnectionTestResult, FieldKind, FieldValidation,
    ProtocolDescriptor, ProtocolProvider, ProviderError, TestStep,
};

use crate::config::{
    DEFAULT_COMMAND_PROMPT, DEFAULT_LOGIN_PROMPT, DEFAULT_PASSWORD_PROMPT, DEFAULT_PORT,
    DEFAULT_TERMINAL_TYPE, DEFAULT_TIMEOUT_SECS, TelnetConfig,
};
use crate::session::{TelnetSession, connect_tcp};

pub struct TelnetProvider {
    descriptor: ProtocolDescriptor,
    schema: ConfigSchema,
}

impl Default for TelnetProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TelnetProvider {
    pub fn new() -> Self {
        Self {
            descriptor: ProtocolDescriptor {
                protocol_id: "telnet".into(),
                display_name: "Telnet".into(),
                version: "1".into(),
                description: "Telnet 明文远程终端（无加密，仅建议在可信网络使用）".into(),
                capabilities: CapabilitySet::of([
                    Capability::Shell,
                    Capability::Exec,
                    Capability::InteractiveTerminal,
                ]),
            },
            schema: build_schema(),
        }
    }
}

fn build_schema() -> ConfigSchema {
    ConfigSchema::new(
        "telnet",
        vec![
            ConfigField {
                key: "host".into(),
                label: "主机".into(),
                kind: FieldKind::Text,
                required: true,
                default_value: None,
                placeholder: Some("192.168.1.1".into()),
                help: Some("设备 IP 地址或主机名".into()),
                secret: false,
                visible_when: None,
                validation: Some(FieldValidation {
                    min_length: Some(1),
                    max_length: Some(255),
                    ..Default::default()
                }),
            },
            ConfigField {
                key: "port".into(),
                label: "端口".into(),
                kind: FieldKind::Number,
                required: false,
                default_value: Some(u64::from(DEFAULT_PORT).into()),
                placeholder: None,
                help: Some("Telnet 默认端口为 23".into()),
                secret: false,
                visible_when: None,
                validation: Some(FieldValidation {
                    min: Some(1),
                    max: Some(65535),
                    ..Default::default()
                }),
            },
            ConfigField {
                key: "username".into(),
                label: "用户名".into(),
                kind: FieldKind::Text,
                required: false,
                default_value: None,
                placeholder: Some("admin".into()),
                help: Some("留空表示设备不需要用户名".into()),
                secret: false,
                visible_when: None,
                validation: Some(FieldValidation {
                    max_length: Some(128),
                    ..Default::default()
                }),
            },
            ConfigField {
                key: "password".into(),
                label: "密码".into(),
                kind: FieldKind::Password,
                required: false,
                default_value: None,
                placeholder: None,
                help: Some("密码由加密存储提供，不会写入连接配置".into()),
                secret: true,
                visible_when: None,
                validation: None,
            },
            ConfigField {
                key: "connection_timeout_secs".into(),
                label: "连接超时（秒）".into(),
                kind: FieldKind::Number,
                required: false,
                default_value: Some(DEFAULT_TIMEOUT_SECS.into()),
                placeholder: None,
                help: None,
                secret: false,
                visible_when: None,
                validation: Some(FieldValidation {
                    min: Some(1),
                    max: Some(120),
                    ..Default::default()
                }),
            },
            ConfigField {
                key: "login_prompt".into(),
                label: "登录提示正则".into(),
                kind: FieldKind::Text,
                required: false,
                default_value: Some(DEFAULT_LOGIN_PROMPT.into()),
                placeholder: None,
                help: Some("用于识别设备要求输入用户名的提示".into()),
                secret: false,
                visible_when: None,
                validation: None,
            },
            ConfigField {
                key: "password_prompt".into(),
                label: "密码提示正则".into(),
                kind: FieldKind::Text,
                required: false,
                default_value: Some(DEFAULT_PASSWORD_PROMPT.into()),
                placeholder: None,
                help: Some("用于识别设备要求输入密码的提示".into()),
                secret: false,
                visible_when: None,
                validation: None,
            },
            ConfigField {
                key: "command_prompt".into(),
                label: "命令提示符正则".into(),
                kind: FieldKind::Text,
                required: false,
                default_value: Some(DEFAULT_COMMAND_PROMPT.into()),
                placeholder: None,
                help: Some("登录成功与命令执行完成的判断依据".into()),
                secret: false,
                visible_when: None,
                validation: None,
            },
            ConfigField {
                key: "terminal_type".into(),
                label: "终端类型".into(),
                kind: FieldKind::Text,
                required: false,
                default_value: Some(DEFAULT_TERMINAL_TYPE.into()),
                placeholder: None,
                help: Some("用于回应设备的 TERMINAL-TYPE 协商".into()),
                secret: false,
                visible_when: None,
                validation: Some(FieldValidation {
                    max_length: Some(64),
                    ..Default::default()
                }),
            },
        ],
    )
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[async_trait]
impl ProtocolProvider for TelnetProvider {
    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> &ConfigSchema {
        &self.schema
    }

    async fn validate_config(&self, config: &ConnectionConfig) -> Result<(), ProviderError> {
        if config.protocol_id != self.descriptor.protocol_id {
            return Err(ProviderError::ConfigInvalid {
                field: "protocol_id".into(),
                message: format!(
                    "配置协议 {} 与 Provider {} 不一致",
                    config.protocol_id, self.descriptor.protocol_id
                ),
            });
        }
        config
            .validate_shape()
            .map_err(|message| ProviderError::ConfigInvalid {
                field: "<root>".into(),
                message,
            })?;
        self.schema.validate(&config.values)?;
        // 三个提示符字段必须能作为正则使用。
        TelnetConfig::validate_patterns(config)
    }

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError> {
        self.validate_config(config).await?;
        // 测试连接接口本身没有 ConnectOptions；App 层会把解密后的机密值合并进 config，
        // 因此这里从 config 读取 password（仅用于测试，不作为生产凭据来源）。
        let mut secrets = std::collections::BTreeMap::new();
        if let Some(password) = config.get_str("password") {
            secrets.insert("password".to_string(), password.to_string());
        }
        let parsed = TelnetConfig::from_config(config, &SecretValues::new(secrets))?;

        let started = Instant::now();
        let stream = match connect_tcp(&parsed).await {
            Ok(stream) => stream,
            Err(err) => {
                let message = err.to_string();
                return Ok(ConnectionTestResult::failure(
                    "Telnet TCP 连接失败",
                    Some(format!("{message}；Telnet 无加密，仅建议在可信网络使用")),
                    vec![TestStep::failed("tcp_connect", message)],
                ));
            }
        };
        let tcp_ms = elapsed_ms(started);
        let mut steps = vec![TestStep::passed("tcp_connect", Some(tcp_ms))];

        let mut session = TelnetSession::from_stream(stream, parsed, SessionId::new());
        let login_result = session.login().await;
        session.shutdown().await;

        match login_result {
            Ok(()) => {
                steps.push(TestStep::passed(
                    "login",
                    Some(elapsed_ms(started).saturating_sub(tcp_ms)),
                ));
                Ok(ConnectionTestResult {
                    ok: true,
                    message: "Telnet 连接成功，已识别命令提示符".into(),
                    latency_ms: Some(elapsed_ms(started)),
                    steps,
                    suggestion: Some("Telnet 无加密，仅建议在可信网络使用".into()),
                    // Telnet 没有 Host Key 概念。
                    host_key: None,
                })
            }
            Err(err) => {
                steps.push(TestStep::failed("login", err.to_string()));
                Ok(ConnectionTestResult::failure(
                    "Telnet 登录或提示符检测失败",
                    Some(
                        "确认用户名、密码与提示符正则配置；需要密码的设备请保存后打开会话验证；\
                         Telnet 无加密，仅建议在可信网络使用"
                            .into(),
                    ),
                    steps,
                ))
            }
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError> {
        self.validate_config(config).await?;
        let mut parsed = TelnetConfig::from_config(config, &options.secrets)?;
        if let Some(terminal_type) = options
            .terminal_type
            .as_deref()
            .map(str::trim)
            .filter(|terminal_type| !terminal_type.is_empty())
        {
            parsed.terminal_type = terminal_type.to_string();
        }
        let session = TelnetSession::open(parsed, SessionId::new()).await?;
        Ok(Box::new(session))
    }
}
