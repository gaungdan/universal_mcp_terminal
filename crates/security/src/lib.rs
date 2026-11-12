//! 安全原语：连接凭据加密（`SecretCipher`）、Token 哈希与常量时间校验（`TokenHasher`）、
//! SSH Host Key 指纹校验（`HostKeyVerifier`）。
//!
//! 独占写入范围：`crates/security/**`。

#![deny(unsafe_code)]
