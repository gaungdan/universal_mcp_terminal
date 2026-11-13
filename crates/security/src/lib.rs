//! 安全原语：连接凭据加密、Token 生成与校验、SSH Host Key 指纹策略。
//!
//! 契约状态：T4 冻结。
//!
//! 设计约束：
//! - 主密钥只存放在操作系统凭据管理器（Windows Credential Manager），不落明文文件；
//! - 连接凭据使用 AES-256-GCM 加密，AAD 固定为 `umt-v1`，防止密文被跨用途重放；
//! - Token 明文只在创建时返回一次，服务端只保存 HMAC-SHA256 与可识别前缀；
//! - 敏感字节统一使用 `zeroize` 清理。

#![deny(unsafe_code)]

pub mod cipher;
pub mod error;
pub mod hostkey;
pub mod keystore;
pub mod token;

pub use cipher::{AesGcmCipher, EncryptedSecret, SecretCipher};
pub use error::SecurityError;
pub use hostkey::{HostKeyDecision, StoredHostKey, evaluate_host_key};
pub use keystore::{KeyStore, OsKeyStore, StaticKeyStore};
pub use token::{TokenHash, TokenSecret, TokenService};

/// AES-GCM 的附加认证数据，禁止在不解密历史数据的前提下修改。
pub const CIPHER_AAD: &[u8] = b"umt-v1";

/// 凭据管理器中主密钥的条目名。
pub const MASTER_KEY_NAME: &str = "secret-master-key";
/// 凭据管理器中 Token 胡椒（pepper）的条目名。
pub const TOKEN_PEPPER_NAME: &str = "token-pepper";
/// 凭据管理器 service 名，与应用 identifier 保持一致。
pub const KEYRING_SERVICE: &str = "com.universal-mcp-terminal";
