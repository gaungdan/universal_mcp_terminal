//! Host Key 指纹校验与 russh 客户端事件处理器。
//!
//! 指纹格式与 OpenSSH 一致：`SHA256:<base64 无填充>`。校验逻辑抽成纯函数，
//! 便于在不发起网络连接的情况下覆盖"未知 / 不匹配 / 一致"三种状态。

use std::sync::{Arc, Mutex};

use russh::keys::{HashAlg, PublicKeyOrCertificate};
use umt_protocol_api::ProviderError;

use crate::error::map_russh_error;

/// 处理器错误：既可能来自 Host Key 校验，也可能来自 russh 自身。
#[derive(Debug, thiserror::Error)]
pub(crate) enum SshHandlerError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Russh(#[from] russh::Error),
}

impl SshHandlerError {
    pub(crate) fn into_provider(self) -> ProviderError {
        match self {
            Self::Provider(err) => err,
            Self::Russh(err) => map_russh_error(err),
        }
    }
}

/// Host Key 校验模式。
pub(crate) enum HostKeyCheck {
    /// 建立正式会话：必须提供预期指纹并严格比对。
    Strict { expected: Option<String> },
    /// 连接测试：接受服务端密钥并记录指纹，供用户确认后保存。
    Discover {
        observed: Arc<Mutex<Option<String>>>,
    },
}

/// russh 客户端事件处理器。
pub(crate) struct SshHandler {
    check: HostKeyCheck,
}

impl SshHandler {
    pub(crate) fn strict(expected: Option<String>) -> Self {
        Self {
            check: HostKeyCheck::Strict { expected },
        }
    }

    pub(crate) fn discover(observed: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            check: HostKeyCheck::Discover { observed },
        }
    }
}

impl russh::client::Handler for SshHandler {
    type Error = SshHandlerError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let actual = fingerprint_of(server_public_key);
        match &mut self.check {
            HostKeyCheck::Strict { expected } => verify_host_key(expected.as_deref(), &actual)
                .map(|()| true)
                .map_err(Into::into),
            HostKeyCheck::Discover { observed } => {
                // 记录指纹；即使锁中毒也不影响握手，用户仍可在测试结果中看到失败。
                if let Ok(mut slot) = observed.lock() {
                    *slot = Some(actual);
                }
                Ok(true)
            }
        }
    }
}

/// 计算服务端公钥的 SHA256 指纹。
pub(crate) fn fingerprint_of(server_public_key: &PublicKeyOrCertificate) -> String {
    server_public_key
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string()
}

/// 三态校验：
/// - 未提供预期指纹 → [`ProviderError::HostKeyUnknown`]（疑似首次连接，需要用户确认）；
/// - 不一致 → [`ProviderError::HostKeyMismatch`]（可能是中间人攻击）；
/// - 一致 → 通过。
pub(crate) fn verify_host_key(expected: Option<&str>, actual: &str) -> Result<(), ProviderError> {
    let actual = actual.trim();
    match expected.map(str::trim).filter(|value| !value.is_empty()) {
        None => Err(ProviderError::HostKeyUnknown {
            fingerprint: actual.to_string(),
        }),
        Some(expected) if expected == actual => Ok(()),
        Some(expected) => Err(ProviderError::HostKeyMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_host_key_is_reported_with_fingerprint() {
        let err = verify_host_key(None, "SHA256:abc123").unwrap_err();
        assert_eq!(
            err,
            ProviderError::HostKeyUnknown {
                fingerprint: "SHA256:abc123".into()
            }
        );
    }

    #[test]
    fn blank_expected_fingerprint_is_treated_as_unknown() {
        let err = verify_host_key(Some("   "), "SHA256:abc123").unwrap_err();
        assert!(matches!(err, ProviderError::HostKeyUnknown { .. }));
    }

    #[test]
    fn mismatched_fingerprint_reports_both_sides() {
        let err = verify_host_key(Some("SHA256:old"), "SHA256:new").unwrap_err();
        assert_eq!(
            err,
            ProviderError::HostKeyMismatch {
                expected: "SHA256:old".into(),
                actual: "SHA256:new".into()
            }
        );
    }

    #[test]
    fn matching_fingerprint_is_accepted() {
        verify_host_key(Some("SHA256:same"), " SHA256:same ").expect("指纹一致应通过");
    }
}
