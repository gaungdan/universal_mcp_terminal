//! 连接敏感字段的加解密与持久化。
//!
//! 明文只以 `BTreeMap<字段名, 值>` 的形式在内存中存在，落库前统一加密；
//! 任何错误信息都不包含字段值。

use std::collections::BTreeMap;

use umt_core::{ConnectionId, UmtError};
use umt_persistence::{SecretBlob, SecretRepo};
use umt_security::{EncryptedSecret, SecretCipher};

/// 把明文敏感字段加密成持久化形态。
pub fn encrypt_secret_map(
    cipher: &dyn SecretCipher,
    secrets: &BTreeMap<String, String>,
) -> Result<SecretBlob, UmtError> {
    let plaintext =
        serde_json::to_vec(secrets).map_err(|_| UmtError::Internal("敏感字段无法序列化".into()))?;
    let encrypted = cipher.encrypt(&plaintext)?;
    Ok(SecretBlob {
        ciphertext: encrypted.ciphertext,
        nonce: encrypted.nonce.to_vec(),
        key_version: encrypted.key_version,
    })
}

/// 把持久化密文还原成明文字段映射。
pub fn decrypt_secret_map(
    cipher: &dyn SecretCipher,
    blob: &SecretBlob,
) -> Result<BTreeMap<String, String>, UmtError> {
    let nonce: [u8; 12] = blob
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| UmtError::Storage("凭据随机数长度非法".into()))?;
    let encrypted = EncryptedSecret {
        ciphertext: blob.ciphertext.clone(),
        nonce,
        key_version: blob.key_version,
    };
    let plaintext = cipher.decrypt(&encrypted)?;
    serde_json::from_slice(plaintext.as_slice())
        .map_err(|_| UmtError::Storage("凭据内容不是合法的字段映射".into()))
}

/// 读取连接的明文字段映射；没有密文时返回空映射。
pub async fn load_secret_map(
    repo: &dyn SecretRepo,
    cipher: &dyn SecretCipher,
    connection_id: ConnectionId,
) -> Result<BTreeMap<String, String>, UmtError> {
    match repo.get(connection_id).await? {
        Some(blob) => decrypt_secret_map(cipher, &blob),
        None => Ok(BTreeMap::new()),
    }
}

/// 写入或清理密文：空映射表示删除已有密文。
pub async fn store_secret_map(
    repo: &dyn SecretRepo,
    cipher: &dyn SecretCipher,
    connection_id: ConnectionId,
    secrets: &BTreeMap<String, String>,
) -> Result<(), UmtError> {
    if secrets.is_empty() {
        repo.delete(connection_id).await?;
        return Ok(());
    }
    let blob = encrypt_secret_map(cipher, secrets)?;
    repo.put(connection_id, blob).await?;
    Ok(())
}
