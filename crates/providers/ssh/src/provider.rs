//! SSH Provider 实现：协议描述、动态配置 Schema、真实连接测试与会话建立。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use russh::client;
use russh::keys::{PrivateKeyWithHashAlg, decode_secret_key};
use tokio::net::TcpStream;
use umt_core::{Capability, CapabilitySet, ConnectionConfig};
use umt_protocol_api::{
    ConfigField, ConfigSchema, ConnectOptions, ConnectionTestResult, FieldCondition, FieldKind,
    FieldValidation, ProtocolDescriptor, ProtocolProvider, ProviderError, SelectOption,
    TerminalSession, TestStep, TestStepStatus,
};

use crate::config::{AuthType, SshAuth, SshConfig, resolve_auth, resolve_auth_from_config};
use crate::error::map_russh_error;
use crate::host_key::SshHandler;
use crate::session::SshSession;

/// SSH 协议 Provider。
pub struct SshProvider {
    descriptor: ProtocolDescriptor,
    schema: ConfigSchema,
}

impl SshProvider {
    pub fn new() -> Self {
        Self {
            descriptor: ProtocolDescriptor {
                protocol_id: "ssh".into(),
                display_name: "SSH".into(),
                version: "1".into(),
                description: "基于 SSH 协议的远程终端与命令执行".into(),
                capabilities: CapabilitySet::of([
                    Capability::Shell,
                    Capability::Exec,
                    Capability::InteractiveTerminal,
                    Capability::TerminalResize,
                ]),
            },
            schema: build_schema(),
        }
    }
}

impl Default for SshProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolProvider for SshProvider {
    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> &ConfigSchema {
        &self.schema
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError> {
        self.validate_config(config).await?;
        let parsed = SshConfig::from_config(config)?;
        // 敏感字段只能来自加密存储解密后的 secrets，绝不从配置对象读取。
        let auth = resolve_auth(&parsed, |key| options.secrets.get(key))?;

        let handler = SshHandler::strict(options.expected_host_key_fingerprint.clone());
        let mut handle = open_connection(&parsed, handler).await?;

        tokio::time::timeout(
            parsed.connection_timeout,
            perform_auth(&mut handle, &parsed.username, &auth),
        )
        .await
        .map_err(|_| ProviderError::Timeout("SSH 认证超时".into()))??;

        let terminal_type = options
            .terminal_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(parsed.terminal_type.as_str());
        let working_dir = options
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(parsed.default_working_dir.as_deref());

        let session = SshSession::open(
            handle,
            &options,
            terminal_type,
            working_dir,
            self.descriptor.capabilities.clone(),
        )
        .await?;
        Ok(Box::new(session))
    }

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError> {
        self.validate_config(config).await?;
        let parsed = SshConfig::from_config(config)?;
        // test_connection 没有 ConnectOptions，只能使用调用方提交的完整表单值。
        let auth = resolve_auth_from_config(&parsed, config)?;

        let started = Instant::now();
        let mut steps = Vec::new();

        // 第 1 步：TCP 连通性。
        let tcp_started = Instant::now();
        let stream = match tokio::time::timeout(
            parsed.connection_timeout,
            TcpStream::connect((parsed.host.as_str(), parsed.port)),
        )
        .await
        {
            Err(_) => {
                steps.push(TestStep::failed("TCP 连接", "连接超时"));
                return Ok(tcp_failure(
                    format!(
                        "无法在 {} 秒内建立 TCP 连接",
                        parsed.connection_timeout.as_secs()
                    ),
                    steps,
                ));
            }
            Ok(Err(err)) => {
                steps.push(TestStep::failed("TCP 连接", err.to_string()));
                return Ok(tcp_failure(format!("TCP 连接失败：{err}"), steps));
            }
            Ok(Ok(stream)) => stream,
        };
        steps.push(TestStep::passed(
            "TCP 连接",
            Some(tcp_started.elapsed().as_millis() as u64),
        ));

        // 第 2 步：SSH 握手并发现 Host Key 指纹（供用户确认后固定保存）。
        let observed = Arc::new(Mutex::new(None));
        let client_config = Arc::new(client::Config {
            keepalive_interval: parsed.keepalive,
            inactivity_timeout: None,
            ..Default::default()
        });
        let handshake_started = Instant::now();
        let handler = SshHandler::discover(observed.clone());
        let mut handle = match tokio::time::timeout(
            parsed.connection_timeout,
            client::connect_stream(client_config, stream, handler),
        )
        .await
        {
            Err(_) => {
                steps.push(TestStep::failed("SSH 握手", "握手超时"));
                return Ok(ConnectionTestResult::failure(
                    "SSH 握手超时",
                    Some("检查目标端口是否为 SSH 服务，或适当增大连接超时".into()),
                    steps,
                ));
            }
            Ok(Err(err)) => {
                let err = err.into_provider();
                steps.push(TestStep::failed("SSH 握手", err.to_string()));
                return Ok(ConnectionTestResult::failure(
                    format!("SSH 握手失败：{err}"),
                    Some("确认目标是 SSH 服务，且客户端与服务端存在共同算法".into()),
                    steps,
                ));
            }
            Ok(Ok(handle)) => handle,
        };
        let fingerprint = observed.lock().ok().and_then(|slot| slot.clone());
        steps.push(TestStep {
            name: "Host Key 校验".into(),
            status: TestStepStatus::Passed,
            message: fingerprint.clone(),
            duration_ms: Some(handshake_started.elapsed().as_millis() as u64),
        });

        // 第 3 步：真实认证。
        let auth_started = Instant::now();
        let auth_result = tokio::time::timeout(
            parsed.connection_timeout,
            perform_auth(&mut handle, &parsed.username, &auth),
        )
        .await;
        match auth_result {
            Err(_) => {
                steps.push(TestStep::failed("SSH 认证", "认证超时"));
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
                return Ok(ConnectionTestResult::failure(
                    "SSH 认证超时",
                    Some("检查服务器负载与网络延迟，或适当增大连接超时".into()),
                    steps,
                ));
            }
            Ok(Err(err)) => {
                steps.push(TestStep::failed("SSH 认证", err.to_string()));
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
                return Ok(ConnectionTestResult::failure(
                    format!("SSH 认证失败：{err}"),
                    Some(auth_suggestion(parsed.auth_type)),
                    steps,
                ));
            }
            Ok(Ok(())) => {}
        }
        steps.push(TestStep::passed(
            "SSH 认证",
            Some(auth_started.elapsed().as_millis() as u64),
        ));
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;

        let message = match fingerprint {
            Some(fingerprint) => format!("SSH 认证成功（Host Key {fingerprint}）"),
            None => "SSH 认证成功".to_string(),
        };
        Ok(ConnectionTestResult::success(
            message,
            started.elapsed().as_millis() as u64,
            steps,
        ))
    }
}

/// 建立 TCP + SSH 握手，超时映射为 [`ProviderError::Timeout`]。
async fn open_connection(
    config: &SshConfig,
    handler: SshHandler,
) -> Result<client::Handle<SshHandler>, ProviderError> {
    let client_config = Arc::new(client::Config {
        keepalive_interval: config.keepalive,
        inactivity_timeout: None,
        ..Default::default()
    });
    let address = (config.host.clone(), config.port);
    match tokio::time::timeout(
        config.connection_timeout,
        client::connect(client_config, address, handler),
    )
    .await
    {
        Err(_) => Err(ProviderError::Timeout(format!(
            "连接 {}:{} 超时",
            config.host, config.port
        ))),
        Ok(Ok(handle)) => Ok(handle),
        Ok(Err(err)) => Err(err.into_provider()),
    }
}

/// 执行密码或公钥认证。错误信息不包含任何凭据内容。
async fn perform_auth(
    handle: &mut client::Handle<SshHandler>,
    username: &str,
    auth: &SshAuth,
) -> Result<(), ProviderError> {
    let result = match auth {
        SshAuth::Password(password) => handle
            .authenticate_password(username, password.as_str())
            .await
            .map_err(map_russh_error)?,
        SshAuth::PrivateKey { pem, passphrase } => {
            let key = decode_secret_key(pem.as_str(), passphrase.as_deref().map(String::as_str))
                .map_err(|_| {
                    ProviderError::AuthFailed("私钥解析失败，请检查私钥格式与口令".into())
                })?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .map_err(map_russh_error)?
                .flatten();
            handle
                .authenticate_publickey(
                    username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await
                .map_err(map_russh_error)?
        }
    };

    if result.success() {
        Ok(())
    } else {
        Err(ProviderError::AuthFailed("服务器拒绝了认证凭据".into()))
    }
}

fn auth_suggestion(auth_type: AuthType) -> String {
    match auth_type {
        AuthType::Password => "检查用户名与密码是否正确，并确认服务器允许密码认证".into(),
        AuthType::PrivateKey => {
            "确认公钥已加入服务器 authorized_keys，并检查私钥口令是否正确".into()
        }
    }
}

/// TCP 连接失败时的统一建议。
fn tcp_failure(message: String, steps: Vec<TestStep>) -> ConnectionTestResult {
    ConnectionTestResult::failure(
        message,
        Some("确认主机地址与端口正确，且网络与防火墙允许访问".into()),
        steps,
    )
}

/// 构造动态表单 Schema。字段 key 已冻结，重命名会破坏既有连接配置。
fn build_schema() -> ConfigSchema {
    let fields = vec![
        ConfigField {
            placeholder: Some("192.168.1.10".into()),
            help: Some("目标服务器的主机名或 IP 地址".into()),
            validation: Some(FieldValidation {
                min_length: Some(1),
                ..Default::default()
            }),
            ..field("host", "主机", FieldKind::Text, true)
        },
        ConfigField {
            default_value: Some(22i64.into()),
            placeholder: Some("22".into()),
            validation: Some(FieldValidation {
                min: Some(1),
                max: Some(65535),
                ..Default::default()
            }),
            ..field("port", "端口", FieldKind::Number, true)
        },
        ConfigField {
            placeholder: Some("root".into()),
            validation: Some(FieldValidation {
                min_length: Some(1),
                ..Default::default()
            }),
            ..field("username", "用户名", FieldKind::Text, true)
        },
        ConfigField {
            kind: FieldKind::Select {
                options: vec![
                    SelectOption {
                        value: "password".into(),
                        label: "密码认证".into(),
                    },
                    SelectOption {
                        value: "private_key".into(),
                        label: "私钥认证".into(),
                    },
                ],
            },
            default_value: Some("password".into()),
            ..field(
                "auth_type",
                "认证方式",
                FieldKind::Select { options: vec![] },
                true,
            )
        },
        ConfigField {
            secret: true,
            visible_when: Some(auth_type_condition("password")),
            ..field("password", "密码", FieldKind::Password, false)
        },
        ConfigField {
            secret: true,
            placeholder: Some("-----BEGIN OPENSSH PRIVATE KEY-----".into()),
            visible_when: Some(auth_type_condition("private_key")),
            ..field("private_key", "私钥", FieldKind::Multiline, false)
        },
        ConfigField {
            secret: true,
            visible_when: Some(auth_type_condition("private_key")),
            ..field(
                "private_key_passphrase",
                "私钥口令",
                FieldKind::Password,
                false,
            )
        },
        ConfigField {
            default_value: Some(15i64.into()),
            validation: Some(FieldValidation {
                min: Some(1),
                max: Some(120),
                ..Default::default()
            }),
            ..field(
                "connection_timeout_secs",
                "连接超时（秒）",
                FieldKind::Number,
                false,
            )
        },
        ConfigField {
            default_value: Some(30i64.into()),
            help: Some("0 表示关闭 keepalive".into()),
            validation: Some(FieldValidation {
                min: Some(0),
                max: Some(600),
                ..Default::default()
            }),
            ..field(
                "keepalive_secs",
                "Keepalive（秒）",
                FieldKind::Number,
                false,
            )
        },
        ConfigField {
            default_value: Some("xterm-256color".into()),
            ..field("terminal_type", "终端类型", FieldKind::Text, false)
        },
        ConfigField {
            placeholder: Some("/home/user".into()),
            ..field(
                "default_working_dir",
                "默认工作目录",
                FieldKind::Text,
                false,
            )
        },
    ];
    ConfigSchema::new("ssh", fields)
}

fn field(key: &str, label: &str, kind: FieldKind, required: bool) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        kind,
        required,
        default_value: None,
        placeholder: None,
        help: None,
        secret: false,
        visible_when: None,
        validation: None,
    }
}

fn auth_type_condition(value: &str) -> FieldCondition {
    FieldCondition {
        key: "auth_type".into(),
        equals: value.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection_config(json: &str) -> ConnectionConfig {
        let values = json.parse().expect("测试 JSON 非法");
        ConnectionConfig::new("ssh", values)
    }

    #[test]
    fn descriptor_declares_expected_capabilities() {
        let provider = SshProvider::new();
        let descriptor = provider.descriptor();
        assert_eq!(descriptor.protocol_id, "ssh");
        assert_eq!(descriptor.display_name, "SSH");
        assert_eq!(descriptor.version, "1");
        for capability in [
            Capability::Shell,
            Capability::Exec,
            Capability::InteractiveTerminal,
            Capability::TerminalResize,
        ] {
            assert!(descriptor.capabilities.contains(capability));
        }
        assert!(!descriptor.capabilities.contains(Capability::PortForward));
        assert!(
            !descriptor
                .capabilities
                .contains(Capability::DeviceInformation)
        );
    }

    #[test]
    fn schema_uses_frozen_field_keys() {
        let provider = SshProvider::new();
        let keys: Vec<_> = provider
            .config_schema()
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![
                "host",
                "port",
                "username",
                "auth_type",
                "password",
                "private_key",
                "private_key_passphrase",
                "connection_timeout_secs",
                "keepalive_secs",
                "terminal_type",
                "default_working_dir",
            ]
        );
    }

    #[test]
    fn secret_fields_and_visibility_match_auth_type() {
        let provider = SshProvider::new();
        let schema = provider.config_schema();
        let password = schema.field("password").expect("缺少 password 字段");
        assert!(password.secret);
        assert_eq!(password.visible_when, Some(auth_type_condition("password")));

        for key in ["private_key", "private_key_passphrase"] {
            let field = schema.field(key).expect("缺少私钥相关字段");
            assert!(field.secret);
            assert_eq!(field.visible_when, Some(auth_type_condition("private_key")));
        }
    }

    #[tokio::test]
    async fn rejects_missing_host() {
        let provider = SshProvider::new();
        let err = provider
            .validate_config(&connection_config(
                r#"{"port":22,"username":"root","auth_type":"password"}"#,
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "host"));
    }

    #[tokio::test]
    async fn rejects_out_of_range_port() {
        let provider = SshProvider::new();
        let err = provider
            .validate_config(&connection_config(
                r#"{"host":"h","port":70000,"username":"root","auth_type":"password"}"#,
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "port"));
    }

    #[tokio::test]
    async fn rejects_unknown_field() {
        let provider = SshProvider::new();
        let err = provider
            .validate_config(&connection_config(
                r#"{"host":"h","port":22,"username":"root","auth_type":"password","inject":true}"#,
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "inject"));
    }

    #[tokio::test]
    async fn accepts_both_auth_type_branches() {
        let provider = SshProvider::new();
        provider
            .validate_config(&connection_config(
                r#"{"host":"h","port":22,"username":"root","auth_type":"password","password":"x"}"#,
            ))
            .await
            .expect("密码认证配置应通过");
        provider
            .validate_config(&connection_config(
                r#"{"host":"h","port":22,"username":"root","auth_type":"private_key","private_key":"-----BEGIN OPENSSH PRIVATE KEY-----"}"#,
            ))
            .await
            .expect("私钥认证配置应通过");
    }

    #[tokio::test]
    async fn rejects_unknown_auth_type() {
        let provider = SshProvider::new();
        let err = provider
            .validate_config(&connection_config(
                r#"{"host":"h","port":22,"username":"root","auth_type":"agent"}"#,
            ))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProviderError::ConfigInvalid { ref field, .. } if field == "auth_type")
        );
    }
}
