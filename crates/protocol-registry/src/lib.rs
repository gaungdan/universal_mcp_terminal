//! 协议注册中心：应用启动时注册 Provider，其余模块只通过 ID 查询。
//!
//! 约束：本 crate 不依赖任何具体协议实现，新增协议只需在启动时 `register`。

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;
use umt_core::UmtError;
use umt_protocol_api::{ConfigSchema, ProtocolDescriptor, ProtocolProvider};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RegistryError {
    #[error("协议 ID 重复注册：{0}")]
    DuplicateProtocol(String),

    #[error("协议 ID 非法：{0}")]
    InvalidProtocolId(String),
}

impl From<RegistryError> for UmtError {
    fn from(value: RegistryError) -> Self {
        UmtError::Conflict(value.to_string())
    }
}

/// 只读协议注册表。注册发生在启动阶段，运行期不再变更。
#[derive(Default)]
pub struct ProtocolRegistry {
    providers: BTreeMap<String, Arc<dyn ProtocolProvider>>,
}

impl ProtocolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: Arc<dyn ProtocolProvider>) -> Result<(), RegistryError> {
        let id = provider.descriptor().protocol_id.clone();
        if id.trim().is_empty() {
            return Err(RegistryError::InvalidProtocolId(id));
        }
        if self.providers.contains_key(&id) {
            return Err(RegistryError::DuplicateProtocol(id));
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    pub fn get(&self, protocol_id: &str) -> Option<Arc<dyn ProtocolProvider>> {
        self.providers.get(protocol_id).cloned()
    }

    pub fn require(&self, protocol_id: &str) -> Result<Arc<dyn ProtocolProvider>, UmtError> {
        self.get(protocol_id).ok_or_else(|| UmtError::NotFound {
            entity: "协议",
            id: protocol_id.to_string(),
        })
    }

    pub fn descriptors(&self) -> Vec<ProtocolDescriptor> {
        self.providers
            .values()
            .map(|p| p.descriptor().clone())
            .collect()
    }

    pub fn schemas(&self) -> Vec<ConfigSchema> {
        self.providers
            .values()
            .map(|p| p.config_schema().clone())
            .collect()
    }

    pub fn protocol_ids(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_protocol_api::MockProvider;

    #[test]
    fn register_and_lookup() {
        let mut registry = ProtocolRegistry::new();
        registry
            .register(Arc::new(MockProvider::new()))
            .expect("注册失败");
        assert_eq!(registry.len(), 1);
        assert!(registry.get("mock").is_some());
        assert!(registry.require("ssh").is_err());
        assert_eq!(registry.schemas()[0].protocol_id, "mock");
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let mut registry = ProtocolRegistry::new();
        registry
            .register(Arc::new(MockProvider::new()))
            .expect("首次注册失败");
        let err = registry
            .register(Arc::new(MockProvider::new()))
            .expect_err("重复注册应失败");
        assert_eq!(err, RegistryError::DuplicateProtocol("mock".into()));
    }
}
