use umt_core::UnixMillis;

/// 已保存的 Host Key 指纹。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHostKey {
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    pub algorithm: Option<String>,
    pub fingerprint: String,
    pub trusted_at: UnixMillis,
    pub trusted_by: Option<String>,
}

/// Host Key 校验结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyDecision {
    /// 指纹一致，允许连接。
    Trusted,
    /// 首次见到的指纹，需要用户在桌面端确认后才能保存。
    Unknown,
    /// 指纹与已保存值不一致，必须阻断并告警。
    Mismatch { expected: String },
}

/// 纯函数策略：不依赖存储与网络，便于测试与复用。
///
/// 无人值守的 MCP 调用在 `Unknown` 时必须拒绝连接并返回错误，
/// 只有桌面端确认流程可以调用保存指纹的操作。
pub fn evaluate_host_key(stored: Option<&StoredHostKey>, presented: &str) -> HostKeyDecision {
    match stored {
        None => HostKeyDecision::Unknown,
        Some(saved) if saved.fingerprint == presented => HostKeyDecision::Trusted,
        Some(saved) => HostKeyDecision::Mismatch {
            expected: saved.fingerprint.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(fingerprint: &str) -> StoredHostKey {
        StoredHostKey {
            endpoint: "ssh://10.0.0.1:22".into(),
            host: "10.0.0.1".into(),
            port: 22,
            algorithm: Some("ssh-ed25519".into()),
            fingerprint: fingerprint.into(),
            trusted_at: UnixMillis::now(),
            trusted_by: None,
        }
    }

    #[test]
    fn unknown_when_nothing_stored() {
        assert_eq!(
            evaluate_host_key(None, "SHA256:abc"),
            HostKeyDecision::Unknown
        );
    }

    #[test]
    fn trusted_when_fingerprint_matches() {
        let saved = stored("SHA256:abc");
        assert_eq!(
            evaluate_host_key(Some(&saved), "SHA256:abc"),
            HostKeyDecision::Trusted
        );
    }

    #[test]
    fn mismatch_when_fingerprint_changed() {
        let saved = stored("SHA256:abc");
        assert_eq!(
            evaluate_host_key(Some(&saved), "SHA256:def"),
            HostKeyDecision::Mismatch {
                expected: "SHA256:abc".into()
            }
        );
    }
}
