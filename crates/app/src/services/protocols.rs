//! 协议列表：把注册中心的描述与配置 Schema 暴露给前端动态表单。

use umt_protocol_api::ConfigSchema;

use crate::ipc::dto::ProtocolInfoDto;
use crate::services::state::AppState;

pub fn list(state: &AppState) -> Vec<ProtocolInfoDto> {
    state
        .protocols
        .descriptors()
        .into_iter()
        .map(|descriptor| {
            let config_schema = state
                .protocols
                .get(&descriptor.protocol_id)
                .map(|provider| provider.config_schema().clone())
                .unwrap_or_else(|| ConfigSchema::new(descriptor.protocol_id.clone(), Vec::new()));
            ProtocolInfoDto {
                protocol_id: descriptor.protocol_id,
                display_name: descriptor.display_name,
                version: descriptor.version,
                description: descriptor.description,
                capabilities: descriptor
                    .capabilities
                    .as_strings()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                config_schema,
            }
        })
        .collect()
}
