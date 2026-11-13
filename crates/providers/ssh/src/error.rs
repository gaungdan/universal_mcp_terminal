//! russh 错误到 [`ProviderError`] 的映射。
//!
//! 错误信息中不得包含密码、私钥或口令；russh 的错误本身不携带这些内容，
//! 这里只做分类，避免把敏感字符串拼接进错误。

use umt_protocol_api::ProviderError;

/// 统一把 russh 错误映射为协议层错误，便于前端按类型给出排错建议。
pub(crate) fn map_russh_error(err: russh::Error) -> ProviderError {
    match err {
        russh::Error::ConnectionTimeout
        | russh::Error::KeepaliveTimeout
        | russh::Error::InactivityTimeout
        | russh::Error::Elapsed(_) => ProviderError::Timeout("SSH 连接超时".into()),
        russh::Error::Disconnect | russh::Error::HUP => {
            ProviderError::Disconnected("SSH 连接已断开".into())
        }
        russh::Error::IO(e) => ProviderError::Network(format!("网络错误：{e}")),
        other => ProviderError::Internal(format!("SSH 协议错误：{other}")),
    }
}
