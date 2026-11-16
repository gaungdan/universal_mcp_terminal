//! 供各 crate 单测使用的 Mock Provider，**不得**在正式运行路径中注册。

use crate::error::{ProviderError, SessionError};
use crate::provider::{ProtocolDescriptor, ProtocolProvider};
use crate::schema::{ConfigField, ConfigSchema, FieldKind, FieldValidation, SelectOption};
use crate::session::{
    ConnectOptions, ExecRequest, ExecResult, FileChunk, FileReadRequest, OutputChunk,
    TerminalSession,
};
use crate::test_result::{ConnectionTestResult, TestStep};
use async_trait::async_trait;
use umt_core::{Capability, CapabilitySet, ConnectionConfig, ExecutionState, SessionId};

pub struct MockProvider {
    descriptor: ProtocolDescriptor,
    schema: ConfigSchema,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    pub fn new() -> Self {
        let schema = ConfigSchema::new(
            "mock",
            vec![
                ConfigField {
                    key: "host".into(),
                    label: "主机".into(),
                    kind: FieldKind::Text,
                    required: true,
                    default_value: None,
                    placeholder: Some("127.0.0.1".into()),
                    help: None,
                    secret: false,
                    visible_when: None,
                    validation: Some(FieldValidation::default()),
                },
                ConfigField {
                    key: "mode".into(),
                    label: "模式".into(),
                    kind: FieldKind::Select {
                        options: vec![
                            SelectOption {
                                value: "ok".into(),
                                label: "正常".into(),
                            },
                            SelectOption {
                                value: "fail".into(),
                                label: "失败".into(),
                            },
                        ],
                    },
                    required: false,
                    default_value: Some(serde_json::json!("ok")),
                    placeholder: None,
                    help: None,
                    secret: false,
                    visible_when: None,
                    validation: None,
                },
            ],
        );

        Self {
            descriptor: ProtocolDescriptor {
                protocol_id: "mock".into(),
                display_name: "Mock".into(),
                version: "1".into(),
                description: "仅用于测试的假协议".into(),
                capabilities: CapabilitySet::of([
                    Capability::Shell,
                    Capability::Exec,
                    Capability::InteractiveTerminal,
                    Capability::TerminalResize,
                    Capability::FileRead,
                ]),
            },
            schema,
        }
    }

    fn should_fail(config: &ConnectionConfig) -> bool {
        config.get_str("mode") == Some("fail") || config.get_str("host") == Some("fail")
    }
}

#[async_trait]
impl ProtocolProvider for MockProvider {
    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> &ConfigSchema {
        &self.schema
    }

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError> {
        self.validate_config(config).await?;
        if Self::should_fail(config) {
            Ok(ConnectionTestResult::failure(
                "模拟连接失败",
                Some("把 mode 改成 ok 再试".into()),
                vec![TestStep::failed("connect", "mock failure")],
            ))
        } else {
            Ok(ConnectionTestResult::success(
                "模拟连接成功",
                1,
                vec![TestStep::passed("connect", Some(1))],
            ))
        }
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError> {
        self.validate_config(config).await?;
        if Self::should_fail(config) {
            return Err(ProviderError::AuthFailed("模拟认证失败".into()));
        }
        let _ = (options.rows, options.cols);
        Ok(Box::new(MockSession {
            id: SessionId::new(),
            capabilities: self.descriptor.capabilities.clone(),
            output: Vec::new(),
            closed: false,
        }))
    }
}

struct MockSession {
    id: SessionId,
    capabilities: CapabilitySet,
    output: Vec<u8>,
    closed: bool,
}

#[async_trait]
impl TerminalSession for MockSession {
    fn session_id(&self) -> SessionId {
        self.id
    }

    fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        self.output.extend_from_slice(data);
        Ok(())
    }

    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        let start = cursor as usize;
        if start >= self.output.len() {
            return Ok(OutputChunk {
                cursor,
                text: String::new(),
                eof: false,
                truncated: false,
            });
        }
        let end = (start + max_bytes).min(self.output.len());
        let text = String::from_utf8_lossy(&self.output[start..end]).into_owned();
        Ok(OutputChunk {
            cursor: end as u64,
            text,
            eof: false,
            truncated: false,
        })
    }

    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        let _ = (rows, cols);
        Ok(())
    }

    async fn read_file(&mut self, request: FileReadRequest) -> Result<FileChunk, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        let content = b"mock-file-content\n";
        let offset = request.offset.min(content.len() as u64) as usize;
        let end = (offset + request.max_bytes.max(1)).min(content.len());
        Ok(FileChunk {
            path: request.path,
            offset: offset as u64,
            next_offset: end as u64,
            size: Some(content.len() as u64),
            eof: end >= content.len(),
            data: content[offset..end].to_vec(),
        })
    }

    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        Ok(ExecResult {
            state: ExecutionState::Succeeded,
            exit_code: Some(0),
            stdout: format!("mock:{}", request.command),
            stderr: String::new(),
            duration_ms: 1,
            truncated: false,
        })
    }

    async fn close(mut self: Box<Self>) -> Result<(), SessionError> {
        self.closed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn mock_provider_roundtrip() {
        let provider = MockProvider::new();
        let config = ConnectionConfig::new("mock", json!({ "host": "local", "mode": "ok" }));
        let mut session = provider
            .connect(&config, ConnectOptions::default())
            .await
            .expect("应能连接");

        session.write(b"hello").await.expect("写入失败");
        let chunk = session.read(0, 1024).await.expect("读取失败");
        assert_eq!(chunk.text, "hello");
        assert_eq!(chunk.cursor, 5);

        let result = session
            .exec(ExecRequest::new("uname"))
            .await
            .expect("执行失败");
        assert_eq!(result.stdout, "mock:uname");
        session.close().await.expect("关闭失败");
    }

    #[tokio::test]
    async fn mock_provider_reports_failure() {
        let provider = MockProvider::new();
        let config = ConnectionConfig::new("mock", json!({ "host": "fail" }));
        let result = provider
            .test_connection(&config)
            .await
            .expect("测试调用失败");
        assert!(!result.ok);
        assert!(
            provider
                .connect(&config, ConnectOptions::default())
                .await
                .is_err()
        );
    }
}
