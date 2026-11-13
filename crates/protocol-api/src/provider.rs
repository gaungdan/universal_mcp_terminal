use crate::error::ProviderError;
use crate::schema::ConfigSchema;
use crate::session::{ConnectOptions, TerminalSession};
use crate::test_result::ConnectionTestResult;
use async_trait::async_trait;
use umt_core::{CapabilitySet, ConnectionConfig};

/// 协议静态描述信息。`protocol_id` 必须全局唯一且一旦发布不可更改。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolDescriptor {
    pub protocol_id: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub capabilities: CapabilitySet,
}

/// 协议统一接口。
///
/// 核心业务只能通过本接口使用协议能力；禁止在核心层出现 `if protocol == "ssh"` 之类的分支。
#[async_trait]
pub trait ProtocolProvider: Send + Sync {
    fn descriptor(&self) -> &ProtocolDescriptor;

    fn config_schema(&self) -> &ConfigSchema;

    /// 默认实现：先校验 JSON 结构，再按 Schema 校验字段。Provider 可覆盖以做更严格的检查。
    async fn validate_config(&self, config: &ConnectionConfig) -> Result<(), ProviderError> {
        if config.protocol_id != self.descriptor().protocol_id {
            return Err(ProviderError::ConfigInvalid {
                field: "protocol_id".into(),
                message: format!(
                    "配置协议 {} 与 Provider {} 不一致",
                    config.protocol_id,
                    self.descriptor().protocol_id
                ),
            });
        }
        config
            .validate_shape()
            .map_err(|message| ProviderError::ConfigInvalid {
                field: "<root>".into(),
                message,
            })?;
        self.config_schema().validate(&config.values)
    }

    /// 真实连接测试：必须完成握手与认证，不能只探测端口。
    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError>;

    /// 建立会话。
    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError>;
}
