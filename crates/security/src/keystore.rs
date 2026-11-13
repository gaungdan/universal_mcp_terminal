use crate::error::SecurityError;
use std::collections::HashMap;
use std::sync::Mutex;
use zeroize::Zeroizing;

/// 32 字节对称密钥的来源。生产实现读写操作系统凭据管理器，测试实现保存在内存。
pub trait KeyStore: Send + Sync {
    /// 取回密钥；不存在时生成、持久化并返回。
    fn get_or_create(&self, name: &str) -> Result<Zeroizing<[u8; 32]>, SecurityError>;

    fn delete(&self, name: &str) -> Result<(), SecurityError>;
}

/// 基于系统凭据管理器（Windows Credential Manager）的密钥存储。
pub struct OsKeyStore {
    service: String,
}

impl Default for OsKeyStore {
    fn default() -> Self {
        Self::new(crate::KEYRING_SERVICE)
    }
}

impl OsKeyStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry, SecurityError> {
        keyring::Entry::new(&self.service, name)
            .map_err(|e| SecurityError::KeyStore(format!("无法打开凭据条目 {name}: {e}")))
    }
}

impl KeyStore for OsKeyStore {
    fn get_or_create(&self, name: &str) -> Result<Zeroizing<[u8; 32]>, SecurityError> {
        let entry = self.entry(name)?;
        match entry.get_secret() {
            Ok(bytes) => {
                let array: [u8; 32] = bytes.try_into().map_err(|_| SecurityError::InvalidKey)?;
                Ok(Zeroizing::new(array))
            }
            Err(keyring::Error::NoEntry) => {
                let key = crate::cipher::random_key()?;
                entry.set_secret(key.as_ref()).map_err(|e| {
                    SecurityError::KeyStore(format!("无法写入凭据条目 {name}: {e}"))
                })?;
                Ok(key)
            }
            Err(e) => Err(SecurityError::KeyStore(format!(
                "无法读取凭据条目 {name}: {e}"
            ))),
        }
    }

    fn delete(&self, name: &str) -> Result<(), SecurityError> {
        let entry = self.entry(name)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecurityError::KeyStore(format!(
                "无法删除凭据条目 {name}: {e}"
            ))),
        }
    }
}

/// 仅用于测试的内存密钥存储。
#[derive(Default)]
pub struct StaticKeyStore {
    keys: Mutex<HashMap<String, [u8; 32]>>,
}

impl StaticKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_key(name: &str, key: [u8; 32]) -> Self {
        let store = Self::default();
        store
            .keys
            .lock()
            .expect("静态密钥库锁被污染")
            .insert(name.to_string(), key);
        store
    }
}

impl KeyStore for StaticKeyStore {
    fn get_or_create(&self, name: &str) -> Result<Zeroizing<[u8; 32]>, SecurityError> {
        let mut guard = self
            .keys
            .lock()
            .map_err(|_| SecurityError::KeyStore("静态密钥库锁被污染".into()))?;
        if let Some(key) = guard.get(name) {
            return Ok(Zeroizing::new(*key));
        }
        let key = *crate::cipher::random_key()?;
        guard.insert(name.to_string(), key);
        Ok(Zeroizing::new(key))
    }

    fn delete(&self, name: &str) -> Result<(), SecurityError> {
        let mut guard = self
            .keys
            .lock()
            .map_err(|_| SecurityError::KeyStore("静态密钥库锁被污染".into()))?;
        guard.remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_store_persists_generated_key() {
        let store = StaticKeyStore::new();
        let first = store.get_or_create("k").expect("生成失败");
        let second = store.get_or_create("k").expect("读取失败");
        assert_eq!(first.as_ref(), second.as_ref());
    }
}
