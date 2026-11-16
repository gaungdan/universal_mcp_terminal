use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use umt_core::{
    Connection, ConnectionId, Page, PageRequest, Token, TokenContext, TokenId, TokenStatus,
    UmtError, UnixMillis,
};
use umt_persistence::{
    ConnectionFilter, ConnectionRepo, GrantRepo, GrantView, SecretBlob, TokenRecord, TokenRepo,
};
use umt_security::{EncryptedSecret, SecretCipher, TokenService};

/// 创建 Token 的结果：明文只在这里出现一次。
pub struct CreatedToken {
    pub token: Token,
    pub plaintext: String,
}

/// 由 Session Manager 实现的会话清理回调，避免 access-control 反向依赖它。
#[async_trait]
pub trait SessionRevoker: Send + Sync {
    /// Token 被禁用/撤销/删除时，终止它名下的活动会话。
    async fn revoke_token_sessions(&self, token_id: TokenId, reason: &str)
    -> Result<u64, UmtError>;

    /// 某条连接授权被关闭时，终止该 (Token, Connection) 组合的活动会话。
    async fn revoke_grant_sessions(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        reason: &str,
    ) -> Result<u64, UmtError>;
}

pub struct AccessControl {
    tokens: Arc<dyn TokenRepo>,
    connections: Arc<dyn ConnectionRepo>,
    grants: Arc<dyn GrantRepo>,
    token_service: TokenService,
    cipher: RwLock<Option<Arc<dyn SecretCipher>>>,
    revoker: RwLock<Option<Arc<dyn SessionRevoker>>>,
}

impl AccessControl {
    pub fn new(
        tokens: Arc<dyn TokenRepo>,
        connections: Arc<dyn ConnectionRepo>,
        grants: Arc<dyn GrantRepo>,
        token_service: TokenService,
    ) -> Self {
        Self {
            tokens,
            connections,
            grants,
            token_service,
            cipher: RwLock::new(None),
            revoker: RwLock::new(None),
        }
    }

    /// 注入凭据加密器后，新建 Token 会自动保存一份加密明文，供本机生成 MCP 配置。
    pub fn set_cipher(&self, cipher: Arc<dyn SecretCipher>) {
        if let Ok(mut guard) = self.cipher.write() {
            *guard = Some(cipher);
        }
    }

    fn cipher(&self) -> Option<Arc<dyn SecretCipher>> {
        self.cipher
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
    }

    /// 启动时注入 Session Manager，之后撤销操作才能联动关闭会话。
    pub fn set_revoker(&self, revoker: Arc<dyn SessionRevoker>) {
        if let Ok(mut guard) = self.revoker.write() {
            *guard = Some(revoker);
        }
    }

    fn revoker(&self) -> Option<Arc<dyn SessionRevoker>> {
        self.revoker
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
    }

    // ---------------- Token 生命周期 ----------------

    pub async fn create_token(
        &self,
        name: &str,
        description: Option<String>,
        expires_at: Option<UnixMillis>,
    ) -> Result<CreatedToken, UmtError> {
        self.create_token_with_permissions(
            name,
            description,
            expires_at,
            umt_core::TokenPermission::ReadOnly,
            false,
        )
        .await
    }

    /// 创建 Token，并显式指定权限与交互式终端开关。
    pub async fn create_token_with_permissions(
        &self,
        name: &str,
        description: Option<String>,
        expires_at: Option<UnixMillis>,
        permission: umt_core::TokenPermission,
        allow_interactive: bool,
    ) -> Result<CreatedToken, UmtError> {
        self.validate_permissions(permission, allow_interactive)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(UmtError::Validation("Token 名称不能为空".into()));
        }
        if let Some(expires_at) = expires_at
            && expires_at <= UnixMillis::now()
        {
            return Err(UmtError::Validation("有效期必须晚于当前时间".into()));
        }

        let (secret, hash) = self.token_service.generate()?;
        let token = Token {
            id: TokenId::new(),
            name: name.to_string(),
            description,
            prefix: hash.prefix,
            status: TokenStatus::Active,
            permission,
            allow_interactive,
            expires_at,
            created_at: UnixMillis::now(),
            last_used_at: None,
        };
        self.tokens
            .create(TokenRecord {
                token: token.clone(),
                hmac: hash.hmac_hex,
            })
            .await?;

        // 保存加密后的明文副本，供本机生成 MCP 配置；未注入加密器时跳过（如单元测试）。
        if let Some(cipher) = self.cipher() {
            let encrypted = cipher.encrypt(secret.expose().as_bytes())?;
            self.tokens
                .put_secret(
                    token.id,
                    SecretBlob {
                        ciphertext: encrypted.ciphertext,
                        nonce: encrypted.nonce.to_vec(),
                        key_version: encrypted.key_version,
                    },
                )
                .await?;
        }

        // 新 Token 默认授权全部现有连接，创建后可在详情中按需关闭。
        self.grant_all_connections(token.id).await?;

        Ok(CreatedToken {
            token,
            plaintext: secret.expose().to_string(),
        })
    }

    fn validate_permissions(
        &self,
        permission: umt_core::TokenPermission,
        allow_interactive: bool,
    ) -> Result<(), UmtError> {
        if permission == umt_core::TokenPermission::ReadOnly && allow_interactive {
            return Err(UmtError::Validation(
                "ReadOnly Token 不允许开启交互式终端，请改用 WriteAccess".into(),
            ));
        }
        Ok(())
    }

    /// 更新 Token 权限与交互式终端开关。
    pub async fn update_token_permissions(
        &self,
        id: TokenId,
        permission: umt_core::TokenPermission,
        allow_interactive: bool,
    ) -> Result<Token, UmtError> {
        self.validate_permissions(permission, allow_interactive)?;
        let current = self.get_token(id).await?;
        if current.status == TokenStatus::Revoked {
            return Err(UmtError::Conflict("Token 已撤销，不能再修改".into()));
        }
        self.tokens
            .set_permissions(id, permission, allow_interactive)
            .await?;
        self.get_token(id).await
    }

    /// 为 Token 批量写入「允许」授权，分页遍历以免连接数量超过单页上限。
    async fn grant_all_connections(&self, token_id: TokenId) -> Result<(), UmtError> {
        let mut offset = 0u32;
        loop {
            let page = self
                .connections
                .list(&ConnectionFilter {
                    page: PageRequest::new(offset, PageRequest::MAX_LIMIT),
                    ..Default::default()
                })
                .await?;
            if page.items.is_empty() {
                break;
            }
            for connection in &page.items {
                self.grants.set(token_id, connection.id, true).await?;
            }
            offset += page.items.len() as u32;
            if u64::from(offset) >= page.total {
                break;
            }
        }
        Ok(())
    }

    /// 读取 Token 明文的加密副本，仅供本地生成 MCP 配置使用。
    ///
    /// 旧版本创建的 Token 没有该副本，返回 `None`。
    pub async fn token_secret(&self, id: TokenId) -> Result<Option<String>, UmtError> {
        let Some(blob) = self.tokens.get_secret(id).await? else {
            return Ok(None);
        };
        let cipher = self
            .cipher()
            .ok_or_else(|| UmtError::Internal("未配置 Token 加密服务".into()))?;
        let nonce: [u8; 12] = blob
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| UmtError::Storage("Token 随机数长度非法".into()))?;
        let encrypted = EncryptedSecret {
            ciphertext: blob.ciphertext,
            nonce,
            key_version: blob.key_version,
        };
        let plaintext = cipher.decrypt(&encrypted)?;
        String::from_utf8(plaintext.to_vec())
            .map(Some)
            .map_err(|_| UmtError::Storage("Token 明文不是合法的 UTF-8".into()))
    }

    pub async fn list_tokens(&self, page: PageRequest) -> Result<Page<Token>, UmtError> {
        Ok(self.tokens.list(page).await?)
    }

    pub async fn get_token(&self, id: TokenId) -> Result<Token, UmtError> {
        self.tokens
            .get(id)
            .await?
            .ok_or_else(|| UmtError::NotFound {
                entity: "Token",
                id: id.to_string(),
            })
    }

    /// 只允许修改名称、描述与有效期；状态变更走 `set_token_status`。
    pub async fn update_token(
        &self,
        id: TokenId,
        name: &str,
        description: Option<String>,
        expires_at: Option<UnixMillis>,
    ) -> Result<Token, UmtError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(UmtError::Validation("Token 名称不能为空".into()));
        }
        let current = self.get_token(id).await?;
        if current.status == TokenStatus::Revoked {
            return Err(UmtError::Conflict("Token 已撤销，不能再修改".into()));
        }
        if let Some(expires_at) = expires_at
            && expires_at <= UnixMillis::now()
        {
            return Err(UmtError::Validation("有效期必须晚于当前时间".into()));
        }
        self.tokens
            .update_metadata(id, name, description.as_deref(), expires_at)
            .await?;
        self.get_token(id).await
    }

    /// 状态迁移：active ↔ disabled，revoked 为终态。
    pub async fn set_token_status(
        &self,
        id: TokenId,
        status: TokenStatus,
    ) -> Result<Token, UmtError> {
        let current = self.get_token(id).await?;
        if current.status == TokenStatus::Revoked && status != TokenStatus::Revoked {
            return Err(UmtError::Conflict("已撤销的 Token 不能恢复".into()));
        }
        self.tokens.set_status(id, status).await?;

        if !status.is_usable()
            && let Some(revoker) = self.revoker()
        {
            let reason = match status {
                TokenStatus::Revoked => "token_revoked",
                TokenStatus::Disabled => "token_disabled",
                TokenStatus::Active => unreachable!("已由上方条件排除"),
            };
            let _ = revoker.revoke_token_sessions(id, reason).await;
        }
        self.get_token(id).await
    }

    /// 删除仅允许针对已停用或已撤销的 Token，避免误删正在使用的凭据。
    pub async fn delete_token(&self, id: TokenId) -> Result<bool, UmtError> {
        let current = self.get_token(id).await?;
        if current.status == TokenStatus::Active {
            return Err(UmtError::Conflict(
                "请先禁用或撤销 Token，再执行删除".into(),
            ));
        }
        if !self.tokens.delete(id).await? {
            return Ok(false);
        }
        self.grants.revoke_all_for_token(id).await?;
        Ok(true)
    }

    // ---------------- 授权关系 ----------------

    /// 返回该 Token 对应的全部连接（含未授权项）。
    pub async fn list_grants(&self, token_id: TokenId) -> Result<Vec<GrantView>, UmtError> {
        self.get_token(token_id).await?;
        Ok(self.grants.list_for_token(token_id).await?)
    }

    pub async fn set_grant(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        allowed: bool,
    ) -> Result<GrantView, UmtError> {
        self.get_token(token_id).await?;
        let connection =
            self.connections
                .get(connection_id)
                .await?
                .ok_or_else(|| UmtError::NotFound {
                    entity: "连接",
                    id: connection_id.to_string(),
                })?;

        self.grants.set(token_id, connection_id, allowed).await?;

        if !allowed && let Some(revoker) = self.revoker() {
            let _ = revoker
                .revoke_grant_sessions(token_id, connection_id, "grant_revoked")
                .await;
        }

        Ok(GrantView {
            connection,
            allowed,
        })
    }

    // ---------------- 认证与授权 ----------------

    /// 校验 Bearer Token。成功返回调用方上下文，并刷新最后使用时间。
    pub async fn authenticate(
        &self,
        bearer: &str,
        client_info: Option<String>,
    ) -> Result<TokenContext, UmtError> {
        let bearer = bearer.trim();
        let prefix = TokenService::prefix_of(bearer)
            .ok_or_else(|| UmtError::AccessDenied("Token 格式无效".into()))?;

        let record = self
            .tokens
            .find_by_prefix(prefix)
            .await?
            .ok_or_else(|| UmtError::AccessDenied("Token 不存在".into()))?;

        if !self.token_service.verify(bearer, &record.hmac) {
            return Err(UmtError::AccessDenied("Token 无效".into()));
        }

        Self::ensure_token_usable(&record.token)?;

        let now = UnixMillis::now();
        self.tokens.touch_last_used(record.token.id, now).await?;
        Ok(
            TokenContext::new(record.token.id, record.token.name.clone())
                .with_permission(record.token.permission)
                .with_interactive(record.token.allow_interactive)
                .with_client_info(client_info),
        )
    }

    /// 每次涉及具体连接的操作都必须调用本方法（七项校验）。
    pub async fn authorize_connection(
        &self,
        context: &TokenContext,
        connection_id: ConnectionId,
    ) -> Result<Connection, UmtError> {
        // 1/2. Token 仍然存在且可用（长会话期间可能被撤销）
        let token = self.get_token(context.token_id).await?;
        Self::ensure_token_usable(&token)?;

        // 3. 连接存在
        let connection =
            self.connections
                .get(connection_id)
                .await?
                .ok_or_else(|| UmtError::NotFound {
                    entity: "连接",
                    id: connection_id.to_string(),
                })?;

        // 4. 连接已启用
        if !connection.enabled {
            return Err(UmtError::AccessDenied(format!(
                "连接 {} 已禁用",
                connection.name
            )));
        }

        // 5. Token 被授权访问该连接
        if !self
            .grants
            .is_allowed(context.token_id, connection_id)
            .await?
        {
            return Err(UmtError::AccessDenied(format!(
                "Token {} 未被授权访问连接 {}",
                context.token_name, connection.name
            )));
        }

        // 6/7. 能力校验由调用方基于 Provider 声明的 CapabilitySet 完成
        Ok(connection)
    }

    fn ensure_token_usable(token: &Token) -> Result<(), UmtError> {
        match token.status {
            TokenStatus::Active => {}
            TokenStatus::Disabled => {
                return Err(UmtError::AccessDenied("Token 已禁用".into()));
            }
            TokenStatus::Revoked => {
                return Err(UmtError::AccessDenied("Token 已撤销".into()));
            }
        }
        if let Some(expires_at) = token.expires_at
            && expires_at <= UnixMillis::now()
        {
            return Err(UmtError::AccessDenied("Token 已过期".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_core::Connection;
    use umt_persistence::InMemoryRepos;
    use umt_security::{AesGcmCipher, StaticKeyStore};

    struct RecordingRevoker {
        token_revoked: std::sync::Mutex<Vec<(TokenId, String)>>,
        grant_revoked: std::sync::Mutex<Vec<(TokenId, ConnectionId, String)>>,
    }

    impl RecordingRevoker {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                token_revoked: std::sync::Mutex::new(Vec::new()),
                grant_revoked: std::sync::Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl SessionRevoker for RecordingRevoker {
        async fn revoke_token_sessions(
            &self,
            token_id: TokenId,
            reason: &str,
        ) -> Result<u64, UmtError> {
            self.token_revoked
                .lock()
                .expect("锁被污染")
                .push((token_id, reason.to_string()));
            Ok(1)
        }

        async fn revoke_grant_sessions(
            &self,
            token_id: TokenId,
            connection_id: ConnectionId,
            reason: &str,
        ) -> Result<u64, UmtError> {
            self.grant_revoked.lock().expect("锁被污染").push((
                token_id,
                connection_id,
                reason.to_string(),
            ));
            Ok(1)
        }
    }

    struct Fixture {
        service: AccessControl,
        repos: InMemoryRepos,
        revoker: Arc<RecordingRevoker>,
    }

    async fn fixture() -> Fixture {
        let repos = InMemoryRepos::new();
        let service = AccessControl::new(
            repos.tokens.clone(),
            repos.connections.clone(),
            repos.grants.clone(),
            TokenService::from_keystore(&StaticKeyStore::new()).expect("装载 pepper 失败"),
        );
        let revoker = RecordingRevoker::new();
        service.set_revoker(revoker.clone());
        let cipher = AesGcmCipher::from_keystore(&StaticKeyStore::new()).expect("装载主密钥失败");
        service.set_cipher(Arc::new(cipher));
        Fixture {
            service,
            repos,
            revoker,
        }
    }

    async fn insert_connection(repos: &InMemoryRepos, name: &str, enabled: bool) -> ConnectionId {
        let id = ConnectionId::new();
        repos
            .connections
            .create(Connection {
                id,
                name: name.to_string(),
                protocol_id: "mock".into(),
                description: None,
                tags: vec![],
                notes: None,
                enabled,
                created_at: UnixMillis::now(),
                updated_at: UnixMillis::now(),
                revision: 1,
            })
            .await
            .expect("插入连接失败");
        id
    }

    #[tokio::test]
    async fn created_token_keeps_encrypted_secret_for_config() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("config", None, None)
            .await
            .expect("创建失败");
        let secret = f
            .service
            .token_secret(created.token.id)
            .await
            .expect("读取失败");
        assert_eq!(secret.as_deref(), Some(created.plaintext.as_str()));
    }

    #[tokio::test]
    async fn new_token_grants_all_connections() {
        let f = fixture().await;
        let prod = insert_connection(&f.repos, "prod", true).await;
        let stage = insert_connection(&f.repos, "stage", false).await;

        let created = f
            .service
            .create_token("auto", None, None)
            .await
            .expect("创建失败");
        let grants = f
            .service
            .list_grants(created.token.id)
            .await
            .expect("读取授权失败");

        assert_eq!(grants.len(), 2);
        assert!(grants.iter().all(|grant| grant.allowed));
        assert!(grants.iter().any(|grant| grant.connection.id == prod));
        assert!(grants.iter().any(|grant| grant.connection.id == stage));
    }

    #[tokio::test]
    async fn created_token_can_authenticate() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        let context = f
            .service
            .authenticate(&created.plaintext, Some("claude-code".into()))
            .await
            .expect("认证失败");
        assert_eq!(context.token_id, created.token.id);
        assert_eq!(context.token_name, "claude");
        let refreshed = f
            .service
            .get_token(created.token.id)
            .await
            .expect("读取失败");
        assert!(refreshed.last_used_at.is_some());
    }

    #[tokio::test]
    async fn wrong_or_missing_token_is_rejected() {
        let f = fixture().await;
        f.service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        assert!(f.service.authenticate("not-a-token", None).await.is_err());
        assert!(
            f.service
                .authenticate("umt_12345678_abcdefghijklmnopqrstuvwxyz0123456789abc", None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn disabled_and_expired_tokens_are_rejected() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        f.service
            .set_token_status(created.token.id, TokenStatus::Disabled)
            .await
            .expect("禁用失败");
        assert!(
            f.service
                .authenticate(&created.plaintext, None)
                .await
                .is_err()
        );

        let expired = f
            .service
            .create_token("expired", None, Some(UnixMillis(UnixMillis::now().0 + 1)))
            .await
            .expect("创建失败");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert!(
            f.service
                .authenticate(&expired.plaintext, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn connection_authorization_follows_grant_and_enabled_state() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        let context = f
            .service
            .authenticate(&created.plaintext, None)
            .await
            .expect("认证失败");
        let connection_id = insert_connection(&f.repos, "prod", true).await;

        // 默认不授权
        assert!(
            f.service
                .authorize_connection(&context, connection_id)
                .await
                .is_err()
        );

        f.service
            .set_grant(created.token.id, connection_id, true)
            .await
            .expect("授权失败");
        f.service
            .authorize_connection(&context, connection_id)
            .await
            .expect("授权后应允许访问");

        // 连接被禁用后立即失效
        f.repos
            .connections
            .set_enabled(connection_id, false)
            .await
            .expect("禁用连接失败");
        assert!(
            f.service
                .authorize_connection(&context, connection_id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn revoking_grants_and_tokens_triggers_session_revoker() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        let connection_id = insert_connection(&f.repos, "prod", true).await;

        f.service
            .set_grant(created.token.id, connection_id, true)
            .await
            .expect("授权失败");
        f.service
            .set_grant(created.token.id, connection_id, false)
            .await
            .expect("取消授权失败");
        assert_eq!(f.revoker.grant_revoked.lock().expect("锁被污染").len(), 1);

        f.service
            .set_token_status(created.token.id, TokenStatus::Revoked)
            .await
            .expect("撤销失败");
        assert_eq!(f.revoker.token_revoked.lock().expect("锁被污染").len(), 1);

        // 撤销是终态
        assert!(
            f.service
                .set_token_status(created.token.id, TokenStatus::Active)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn active_token_cannot_be_deleted() {
        let f = fixture().await;
        let created = f
            .service
            .create_token("claude", None, None)
            .await
            .expect("创建失败");
        assert!(f.service.delete_token(created.token.id).await.is_err());
        f.service
            .set_token_status(created.token.id, TokenStatus::Disabled)
            .await
            .expect("禁用失败");
        assert!(
            f.service
                .delete_token(created.token.id)
                .await
                .expect("删除失败")
        );
    }
}
