use crate::error::SecurityError;
use crate::keystore::KeyStore;
use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload, array::Array};
use rand::Rng;
use zeroize::Zeroizing;

/// 加密后的凭据。`nonce` 与 `key_version` 必须与密文一起持久化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
    pub key_version: u32,
}

/// 凭据加解密接口。
pub trait SecretCipher: Send + Sync {
    fn key_version(&self) -> u32;

    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedSecret, SecurityError>;

    fn decrypt(&self, secret: &EncryptedSecret) -> Result<Zeroizing<Vec<u8>>, SecurityError>;
}

/// AES-256-GCM 实现。
pub struct AesGcmCipher {
    key: Zeroizing<[u8; 32]>,
    key_version: u32,
}

impl AesGcmCipher {
    pub fn new(key: Zeroizing<[u8; 32]>, key_version: u32) -> Self {
        Self { key, key_version }
    }

    /// 从系统密钥库装载主密钥。第一次调用会自动生成。
    pub fn from_keystore(store: &dyn KeyStore) -> Result<Self, SecurityError> {
        let key = store.get_or_create(crate::MASTER_KEY_NAME)?;
        Ok(Self::new(key, 1))
    }

    fn cipher(&self) -> Aes256Gcm {
        let key: Array<u8, aes_gcm::aead::consts::U32> = Array::from(*self.key);
        Aes256Gcm::new(&key)
    }
}

impl SecretCipher for AesGcmCipher {
    fn key_version(&self) -> u32 {
        self.key_version
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedSecret, SecurityError> {
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce: Array<u8, aes_gcm::aead::consts::U12> = Array::from(nonce_bytes);
        let ciphertext = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: crate::CIPHER_AAD,
                },
            )
            .map_err(|_| SecurityError::Crypto("加密失败".into()))?;
        Ok(EncryptedSecret {
            ciphertext,
            nonce: nonce_bytes,
            key_version: self.key_version,
        })
    }

    fn decrypt(&self, secret: &EncryptedSecret) -> Result<Zeroizing<Vec<u8>>, SecurityError> {
        if secret.key_version != self.key_version {
            return Err(SecurityError::Crypto(format!(
                "密钥版本不匹配：密文 {}，当前 {}",
                secret.key_version, self.key_version
            )));
        }
        let nonce: Array<u8, aes_gcm::aead::consts::U12> = Array::from(secret.nonce);
        let plaintext = self
            .cipher()
            .decrypt(
                &nonce,
                Payload {
                    msg: secret.ciphertext.as_slice(),
                    aad: crate::CIPHER_AAD,
                },
            )
            .map_err(|_| SecurityError::CorruptedSecret)?;
        Ok(Zeroizing::new(plaintext))
    }
}

/// 生成 32 字节随机密钥。
pub fn random_key() -> Result<Zeroizing<[u8; 32]>, SecurityError> {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    Ok(Zeroizing::new(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> AesGcmCipher {
        AesGcmCipher::new(Zeroizing::new([7u8; 32]), 1)
    }

    #[test]
    fn roundtrip_succeeds() {
        let cipher = cipher();
        let secret = cipher.encrypt(b"super-secret-password").expect("加密失败");
        let plaintext = cipher.decrypt(&secret).expect("解密失败");
        assert_eq!(plaintext.as_slice(), b"super-secret-password");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let cipher = cipher();
        let mut secret = cipher.encrypt(b"secret").expect("加密失败");
        secret.ciphertext[0] ^= 0xff;
        assert!(cipher.decrypt(&secret).is_err());
    }

    #[test]
    fn different_nonce_per_encryption() {
        let cipher = cipher();
        let a = cipher.encrypt(b"same").expect("加密失败");
        let b = cipher.encrypt(b"same").expect("加密失败");
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }
}
