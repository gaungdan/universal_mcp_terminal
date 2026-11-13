use crate::error::SecurityError;
use crate::keystore::KeyStore;
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// 创建成功后仅返回一次的 Token 明文。
pub struct TokenSecret(Zeroizing<String>);

impl TokenSecret {
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

/// 服务端持久化的 Token 校验信息：可识别前缀 + HMAC。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenHash {
    pub prefix: String,
    pub hmac_hex: String,
}

/// Token 生成与校验服务。
///
/// Token 结构：`umt_<8位十六进制前缀>_<32 字节随机数的 hex>`，总熵 256 bit。
/// 随机体使用 hex 而不是 base64url，因为后者的字母表含 `_`，会破坏分隔符解析。
pub struct TokenService {
    pepper: Zeroizing<[u8; 32]>,
}

impl TokenService {
    pub fn new(pepper: Zeroizing<[u8; 32]>) -> Self {
        Self { pepper }
    }

    /// 从系统密钥库装载 pepper，不存在时自动生成。
    pub fn from_keystore(store: &dyn KeyStore) -> Result<Self, SecurityError> {
        Ok(Self::new(store.get_or_create(crate::TOKEN_PEPPER_NAME)?))
    }

    pub fn generate(&self) -> Result<(TokenSecret, TokenHash), SecurityError> {
        let mut raw = [0u8; 32];
        rand::rng().fill_bytes(&mut raw);
        let body = hex::encode(raw);
        let prefix = body.chars().take(8).collect::<String>();
        let token = format!("umt_{prefix}_{body}");
        let hmac_hex = self.hash(&token)?;
        Ok((
            TokenSecret(Zeroizing::new(token)),
            TokenHash { prefix, hmac_hex },
        ))
    }

    pub fn hash(&self, token: &str) -> Result<String, SecurityError> {
        let mut mac = <HmacSha256 as KeyInit>::new_from_slice(self.pepper.as_ref())
            .map_err(|_| SecurityError::InvalidKey)?;
        mac.update(token.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    /// 常量时间比较，避免时序侧信道。
    pub fn verify(&self, token: &str, stored_hmac_hex: &str) -> bool {
        let Ok(computed) = self.hash(token) else {
            return false;
        };
        let computed = computed.as_bytes();
        let stored = stored_hmac_hex.as_bytes();
        computed.len() == stored.len() && computed.ct_eq(stored).into()
    }

    /// 提取 Token 中用于索引的前缀；格式非法返回 None。
    ///
    /// 使用 `splitn(3, '_')` 保证第三段完整保留（随机体不含 `_`，但保持解析健壮）。
    pub fn prefix_of(token: &str) -> Option<&str> {
        let mut parts = token.splitn(3, '_');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("umt"), Some(prefix), Some(body)) if prefix.len() == 8 && body.len() >= 40 => {
                Some(prefix)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::StaticKeyStore;

    fn service() -> TokenService {
        TokenService::new(Zeroizing::new([9u8; 32]))
    }

    #[test]
    fn generated_token_verifies() {
        let service = service();
        let (secret, hash) = service.generate().expect("生成失败");
        assert!(service.verify(secret.expose(), &hash.hmac_hex));
        assert_eq!(
            TokenService::prefix_of(secret.expose()),
            Some(hash.prefix.as_str())
        );
    }

    #[test]
    fn wrong_token_is_rejected() {
        let service = service();
        let (_, hash) = service.generate().expect("生成失败");
        assert!(!service.verify("umt_12345678_wrong", &hash.hmac_hex));
    }

    #[test]
    fn different_pepper_cannot_verify() {
        let other = TokenService::new(Zeroizing::new([1u8; 32]));
        let (secret, hash) = service().generate().expect("生成失败");
        assert!(!other.verify(secret.expose(), &hash.hmac_hex));
    }

    #[test]
    fn token_has_256_bit_entropy_source() {
        let service = service();
        let (secret, _) = service.generate().expect("生成失败");
        let body = secret.expose().splitn(3, '_').nth(2).expect("缺少随机体");
        assert_eq!(body.len(), 64);
    }

    #[test]
    fn keystore_provides_stable_pepper() {
        let store = StaticKeyStore::new();
        let a = TokenService::from_keystore(&store).expect("装载失败");
        let b = TokenService::from_keystore(&store).expect("装载失败");
        let (secret, hash) = a.generate().expect("生成失败");
        assert!(b.verify(secret.expose(), &hash.hmac_hex));
    }
}
