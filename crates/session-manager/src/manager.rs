use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use umt_access_control::SessionRevoker;
use umt_audit::AuditService;
use umt_core::{
    AuditKind, AuditOutcome, Connection, ConnectionConfig, ConnectionId, ExecutionState, Page,
    PageRequest, SessionId, SessionInfo, SessionState, TokenContext, TokenId, UmtError, UnixMillis,
};
use umt_persistence::{NewAuditEntry, SessionFilter, SessionRepo};
use umt_protocol_api::{
    ConnectOptions, ExecRequest, ExecResult, OutputChunk, ProtocolProvider, TerminalSession,
};

/// 建立会话的入参。连接与配置必须已经过 AccessControl 授权。
pub struct OpenSessionRequest {
    pub context: TokenContext,
    pub connection: Connection,
    pub config: ConnectionConfig,
    pub secrets: umt_protocol_api::SecretValues,
    pub expected_host_key_fingerprint: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub terminal_type: Option<String>,
    pub working_dir: Option<String>,
}

/// 会话数量与空闲超时限制。
#[derive(Debug, Clone, Copy)]
pub struct SessionLimits {
    pub max_sessions: usize,
    pub idle_timeout: Duration,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            max_sessions: 32,
            idle_timeout: Duration::from_secs(30 * 60),
        }
    }
}

struct ActiveSession {
    info: SessionInfo,
    session: Mutex<Box<dyn TerminalSession>>,
    last_activity: AtomicI64,
    running: std::sync::atomic::AtomicBool,
}

/// 会话管理器。所有方法都要求调用方传入 Token 上下文，跨 Token 操作一律拒绝。
pub struct SessionManager {
    repo: Arc<dyn SessionRepo>,
    audit: Arc<AuditService>,
    active: DashMap<SessionId, Arc<ActiveSession>>,
    limits: SessionLimits,
}

impl SessionManager {
    pub fn new(
        repo: Arc<dyn SessionRepo>,
        audit: Arc<AuditService>,
        limits: SessionLimits,
    ) -> Self {
        Self {
            repo,
            audit,
            active: DashMap::new(),
            limits,
        }
    }

    /// 建立会话。Provider 必须由调用方从注册中心取得。
    pub async fn open(
        &self,
        provider: Arc<dyn ProtocolProvider>,
        request: OpenSessionRequest,
    ) -> Result<SessionInfo, UmtError> {
        if self.active.len() >= self.limits.max_sessions {
            return Err(UmtError::Conflict(format!(
                "活动会话数已达上限 {}",
                self.limits.max_sessions
            )));
        }
        if !request.connection.enabled {
            return Err(UmtError::AccessDenied("连接已禁用".into()));
        }

        let options = ConnectOptions {
            rows: request.rows,
            cols: request.cols,
            terminal_type: request.terminal_type.clone(),
            working_dir: request.working_dir.clone(),
            env: Vec::new(),
            timeout_ms: 15_000,
            secrets: request.secrets.clone(),
            expected_host_key_fingerprint: request.expected_host_key_fingerprint.clone(),
        };

        let session = provider.connect(&request.config, options).await?;
        let now = UnixMillis::now();
        let info = SessionInfo {
            id: session.session_id(),
            token_id: request.context.token_id,
            connection_id: request.connection.id,
            protocol_id: provider.descriptor().protocol_id.clone(),
            client_info: request.context.client_info.clone(),
            state: SessionState::Connected,
            started_at: now,
            ended_at: None,
            close_reason: None,
            rows: request.rows,
            cols: request.cols,
        };
        self.repo.create(&info).await?;

        self.active.insert(
            info.id,
            Arc::new(ActiveSession {
                info: info.clone(),
                session: Mutex::new(session),
                last_activity: AtomicI64::new(now.0),
                running: std::sync::atomic::AtomicBool::new(false),
            }),
        );

        self.audit
            .record(NewAuditEntry {
                session_id: Some(info.id),
                token_id: Some(info.token_id),
                connection_id: Some(info.connection_id),
                kind: AuditKind::SessionOpened,
                outcome: AuditOutcome::Allowed,
                command: None,
                working_dir: request.working_dir,
                started_at: now,
                duration_ms: None,
                state: ExecutionState::Succeeded,
                exit_code: None,
                stdout_excerpt: None,
                stderr_excerpt: None,
                error: None,
                redacted: false,
            })
            .await?;

        Ok(info)
    }

    pub async fn write(
        &self,
        session_id: SessionId,
        token_id: TokenId,
        data: &[u8],
    ) -> Result<(), UmtError> {
        let session = self.owned(session_id, token_id)?;
        let mut guard = session.session.lock().await;
        guard.write(data).await?;
        drop(guard);
        self.touch(&session);
        self.promote_to_running(&session).await;
        // 交互式输入按时间线记录；内容会在 AuditService 内再次脱敏。
        let input = String::from_utf8_lossy(data).into_owned();
        self.audit
            .record(NewAuditEntry {
                command: Some(input),
                ..self.event(
                    &session,
                    AuditKind::InputSent,
                    ExecutionState::Succeeded,
                    None,
                )
            })
            .await?;
        Ok(())
    }

    pub async fn read(
        &self,
        session_id: SessionId,
        token_id: TokenId,
        cursor: u64,
        max_bytes: usize,
    ) -> Result<OutputChunk, UmtError> {
        let session = self.owned(session_id, token_id)?;
        let mut guard = session.session.lock().await;
        let chunk = guard.read(cursor, max_bytes).await?;
        drop(guard);
        self.touch(&session);
        Ok(chunk)
    }

    pub async fn resize(
        &self,
        session_id: SessionId,
        token_id: TokenId,
        rows: u16,
        cols: u16,
    ) -> Result<(), UmtError> {
        let session = self.owned(session_id, token_id)?;
        let mut guard = session.session.lock().await;
        guard.resize(rows, cols).await?;
        drop(guard);
        self.touch(&session);
        Ok(())
    }

    /// 执行非交互命令并记录审计（含输出摘要与执行状态）。
    pub async fn exec(
        &self,
        session_id: SessionId,
        token_id: TokenId,
        request: ExecRequest,
    ) -> Result<ExecResult, UmtError> {
        let session = self.owned(session_id, token_id)?;
        let started = UnixMillis::now();
        let command = request.command.clone();
        let mut guard = session.session.lock().await;
        let result = guard.exec(request).await;
        drop(guard);
        self.touch(&session);
        self.promote_to_running(&session).await;

        let elapsed = (UnixMillis::now().0 - started.0).max(0) as u64;
        match result {
            Ok(result) => {
                self.audit
                    .record(NewAuditEntry {
                        session_id: Some(session_id),
                        token_id: Some(token_id),
                        connection_id: Some(session.info.connection_id),
                        kind: AuditKind::CommandExecuted,
                        outcome: match result.state {
                            ExecutionState::Succeeded => AuditOutcome::Allowed,
                            _ => AuditOutcome::Failed,
                        },
                        command: Some(command),
                        working_dir: None,
                        started_at: started,
                        duration_ms: Some(result.duration_ms.max(elapsed)),
                        state: result.state,
                        exit_code: result.exit_code,
                        stdout_excerpt: Some(result.stdout.clone()),
                        stderr_excerpt: Some(result.stderr.clone()),
                        error: None,
                        redacted: false,
                    })
                    .await?;
                Ok(result)
            }
            Err(err) => {
                let umt: UmtError = err.into();
                self.audit
                    .record(NewAuditEntry {
                        session_id: Some(session_id),
                        token_id: Some(token_id),
                        connection_id: Some(session.info.connection_id),
                        kind: AuditKind::CommandExecuted,
                        outcome: AuditOutcome::Failed,
                        command: Some(command),
                        working_dir: None,
                        started_at: started,
                        duration_ms: Some(elapsed),
                        state: ExecutionState::Unknown,
                        exit_code: None,
                        stdout_excerpt: None,
                        stderr_excerpt: None,
                        error: Some(umt.to_string()),
                        redacted: false,
                    })
                    .await?;
                Err(umt)
            }
        }
    }

    pub async fn close(
        &self,
        session_id: SessionId,
        token_id: TokenId,
        reason: &str,
    ) -> Result<bool, UmtError> {
        self.owned(session_id, token_id)?;
        self.close_inner(session_id, reason).await
    }

    pub async fn list_active(&self, page: PageRequest) -> Result<Page<SessionInfo>, UmtError> {
        Ok(self.repo.list_active(page).await?)
    }

    pub async fn list_history(
        &self,
        filter: &SessionFilter,
    ) -> Result<Page<SessionInfo>, UmtError> {
        Ok(self.repo.list_history(filter).await?)
    }

    /// 关闭超时未活动的会话，返回关闭数量。由 App 层定时调用。
    pub async fn reap_idle(&self) -> u64 {
        if self.limits.idle_timeout.is_zero() {
            return 0;
        }
        let deadline = UnixMillis::now().0 - self.limits.idle_timeout.as_millis() as i64;
        let stale: Vec<SessionId> = self
            .active
            .iter()
            .filter(|entry| entry.value().last_activity.load(Ordering::Relaxed) < deadline)
            .map(|entry| *entry.key())
            .collect();

        let mut closed = 0;
        for id in stale {
            if self.close_inner(id, "idle_timeout").await.unwrap_or(false) {
                closed += 1;
            }
        }
        closed
    }

    fn owned(
        &self,
        session_id: SessionId,
        token_id: TokenId,
    ) -> Result<Arc<ActiveSession>, UmtError> {
        let entry = self
            .active
            .get(&session_id)
            .ok_or_else(|| UmtError::NotFound {
                entity: "会话",
                id: session_id.to_string(),
            })?;
        if entry.info.token_id != token_id {
            return Err(UmtError::AccessDenied("不能操作其他 Token 的会话".into()));
        }
        Ok(Arc::clone(entry.value()))
    }

    fn touch(&self, session: &ActiveSession) {
        session
            .last_activity
            .store(UnixMillis::now().0, Ordering::Relaxed);
    }

    async fn promote_to_running(&self, session: &ActiveSession) {
        if session.running.swap(true, Ordering::Relaxed) {
            return;
        }
        self.repo
            .update_state(session.info.id, SessionState::Running, None, None)
            .await
            .ok();
    }

    fn event(
        &self,
        session: &ActiveSession,
        kind: AuditKind,
        state: ExecutionState,
        command: Option<String>,
    ) -> NewAuditEntry {
        NewAuditEntry {
            session_id: Some(session.info.id),
            token_id: Some(session.info.token_id),
            connection_id: Some(session.info.connection_id),
            kind,
            outcome: AuditOutcome::Allowed,
            command,
            working_dir: None,
            started_at: UnixMillis::now(),
            duration_ms: None,
            state,
            exit_code: None,
            stdout_excerpt: None,
            stderr_excerpt: None,
            error: None,
            redacted: false,
        }
    }

    async fn close_inner(&self, session_id: SessionId, reason: &str) -> Result<bool, UmtError> {
        let Some((_, active)) = self.active.remove(&session_id) else {
            return Ok(false);
        };
        let session = match Arc::try_unwrap(active) {
            Ok(session) => session,
            Err(shared) => {
                // 仍有调用方持有引用时，至少把状态落库，避免记录悬挂。
                self.repo
                    .update_state(
                        session_id,
                        SessionState::Closed,
                        Some(UnixMillis::now()),
                        Some(reason.to_string()),
                    )
                    .await?;
                let _ = shared;
                return Ok(true);
            }
        };

        let mut guard = session.session.lock().await;
        let terminal = std::mem::replace(&mut *guard, Box::new(ClosedSession));
        let close_result = terminal.close().await;
        drop(guard);

        self.repo
            .update_state(
                session_id,
                SessionState::Closed,
                Some(UnixMillis::now()),
                Some(reason.to_string()),
            )
            .await?;

        self.audit
            .record(NewAuditEntry {
                session_id: Some(session_id),
                token_id: Some(session.info.token_id),
                connection_id: Some(session.info.connection_id),
                kind: AuditKind::SessionClosed,
                outcome: AuditOutcome::Allowed,
                command: None,
                working_dir: None,
                started_at: UnixMillis::now(),
                duration_ms: None,
                state: ExecutionState::Succeeded,
                exit_code: None,
                stdout_excerpt: None,
                stderr_excerpt: None,
                error: close_result.err().map(|e| e.to_string()),
                redacted: false,
            })
            .await?;

        Ok(true)
    }
}

#[async_trait]
impl SessionRevoker for SessionManager {
    async fn revoke_token_sessions(
        &self,
        token_id: TokenId,
        reason: &str,
    ) -> Result<u64, UmtError> {
        let ids: Vec<SessionId> = self
            .active
            .iter()
            .filter(|entry| entry.value().info.token_id == token_id)
            .map(|entry| *entry.key())
            .collect();
        let mut closed = 0;
        for id in ids {
            if self.close_inner(id, reason).await? {
                closed += 1;
            }
        }
        Ok(closed)
    }

    async fn revoke_grant_sessions(
        &self,
        token_id: TokenId,
        connection_id: ConnectionId,
        reason: &str,
    ) -> Result<u64, UmtError> {
        let ids: Vec<SessionId> = self
            .active
            .iter()
            .filter(|entry| {
                entry.value().info.token_id == token_id
                    && entry.value().info.connection_id == connection_id
            })
            .map(|entry| *entry.key())
            .collect();
        let mut closed = 0;
        for id in ids {
            if self.close_inner(id, reason).await? {
                closed += 1;
            }
        }
        Ok(closed)
    }
}

/// 关闭时的占位实现，确保 `Box<dyn TerminalSession>` 被移出后仍有合法值。
struct ClosedSession;

#[async_trait]
impl TerminalSession for ClosedSession {
    fn session_id(&self) -> SessionId {
        SessionId::default()
    }

    fn capabilities(&self) -> &umt_core::CapabilitySet {
        static EMPTY: std::sync::OnceLock<umt_core::CapabilitySet> = std::sync::OnceLock::new();
        EMPTY.get_or_init(umt_core::CapabilitySet::empty)
    }

    async fn write(&mut self, _data: &[u8]) -> Result<(), umt_protocol_api::SessionError> {
        Err(umt_protocol_api::SessionError::Closed)
    }

    async fn read(
        &mut self,
        _cursor: u64,
        _max_bytes: usize,
    ) -> Result<OutputChunk, umt_protocol_api::SessionError> {
        Err(umt_protocol_api::SessionError::Closed)
    }

    async fn resize(
        &mut self,
        _rows: u16,
        _cols: u16,
    ) -> Result<(), umt_protocol_api::SessionError> {
        Err(umt_protocol_api::SessionError::Closed)
    }

    async fn close(self: Box<Self>) -> Result<(), umt_protocol_api::SessionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use umt_core::{Connection, TokenId};
    use umt_persistence::{AuditRepo, ConnectionRepo, InMemoryRepos};
    use umt_protocol_api::{MockProvider, ProtocolProvider, SecretValues};

    struct Fixture {
        manager: SessionManager,
        repos: InMemoryRepos,
        provider: Arc<dyn ProtocolProvider>,
        token_a: TokenId,
        token_b: TokenId,
        connection: Connection,
    }

    async fn fixture(limits: SessionLimits) -> Fixture {
        let repos = InMemoryRepos::new();
        let audit = Arc::new(AuditService::new(repos.audit.clone()));
        let manager = SessionManager::new(repos.sessions.clone(), audit, limits);

        let connection = Connection {
            id: ConnectionId::new(),
            name: "mock".into(),
            protocol_id: "mock".into(),
            description: None,
            tags: vec![],
            notes: None,
            enabled: true,
            created_at: UnixMillis::now(),
            updated_at: UnixMillis::now(),
            revision: 1,
        };
        repos
            .connections
            .create(connection.clone())
            .await
            .expect("插入连接失败");

        Fixture {
            manager,
            repos,
            provider: Arc::new(MockProvider::new()),
            token_a: TokenId::new(),
            token_b: TokenId::new(),
            connection,
        }
    }

    fn request(f: &Fixture, token_id: TokenId) -> OpenSessionRequest {
        OpenSessionRequest {
            context: TokenContext::new(token_id, "test-token"),
            connection: f.connection.clone(),
            config: ConnectionConfig::new("mock", serde_json::json!({ "host": "local" })),
            secrets: SecretValues::default(),
            expected_host_key_fingerprint: None,
            rows: 24,
            cols: 80,
            terminal_type: None,
            working_dir: None,
        }
    }

    #[tokio::test]
    async fn open_write_read_exec_close() {
        let f = fixture(SessionLimits::default()).await;
        let info = f
            .manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("打开会话失败");
        assert_eq!(info.state, SessionState::Connected);

        f.manager
            .write(info.id, f.token_a, b"echo hi")
            .await
            .expect("写入失败");
        let chunk = f
            .manager
            .read(info.id, f.token_a, 0, 1024)
            .await
            .expect("读取失败");
        assert_eq!(chunk.text, "echo hi");

        let result = f
            .manager
            .exec(info.id, f.token_a, ExecRequest::new("uname"))
            .await
            .expect("执行失败");
        assert_eq!(result.stdout, "mock:uname");

        assert!(
            f.manager
                .close(info.id, f.token_a, "user")
                .await
                .expect("关闭失败")
        );
        let stored = f
            .repos
            .sessions
            .get(info.id)
            .await
            .expect("读取会话失败")
            .expect("会话应存在");
        assert_eq!(stored.state, SessionState::Closed);

        let audits = f.repos.audit.count().await.expect("统计审计失败");
        assert!(audits >= 4, "应记录打开/输入/执行/关闭，实际 {audits}");
    }

    #[tokio::test]
    async fn other_token_cannot_touch_session() {
        let f = fixture(SessionLimits::default()).await;
        let info = f
            .manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("打开会话失败");

        assert!(f.manager.write(info.id, f.token_b, b"x").await.is_err());
        assert!(f.manager.close(info.id, f.token_b, "steal").await.is_err());
    }

    #[tokio::test]
    async fn revoking_token_closes_its_sessions() {
        let f = fixture(SessionLimits::default()).await;
        let info = f
            .manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("打开会话失败");

        let closed = f
            .manager
            .revoke_token_sessions(f.token_a, "token_revoked")
            .await
            .expect("撤销失败");
        assert_eq!(closed, 1);

        let stored = f
            .repos
            .sessions
            .get(info.id)
            .await
            .expect("读取失败")
            .expect("会话应存在");
        assert_eq!(stored.close_reason.as_deref(), Some("token_revoked"));
    }

    #[tokio::test]
    async fn idle_sessions_are_reaped() {
        let f = fixture(SessionLimits {
            max_sessions: 8,
            idle_timeout: Duration::from_millis(1),
        })
        .await;
        let info = f
            .manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("打开会话失败");

        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(f.manager.reap_idle().await, 1);

        let stored = f
            .repos
            .sessions
            .get(info.id)
            .await
            .expect("读取失败")
            .expect("会话应存在");
        assert_eq!(stored.close_reason.as_deref(), Some("idle_timeout"));
    }

    #[tokio::test]
    async fn max_sessions_limit_is_enforced() {
        let f = fixture(SessionLimits {
            max_sessions: 1,
            idle_timeout: Duration::from_secs(3600),
        })
        .await;
        f.manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("第一个会话应成功");
        assert!(
            f.manager
                .open(f.provider.clone(), request(&f, f.token_a))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn mock_capabilities_are_exposed() {
        let f = fixture(SessionLimits::default()).await;
        let info = f
            .manager
            .open(f.provider.clone(), request(&f, f.token_a))
            .await
            .expect("打开会话失败");
        assert_eq!(info.protocol_id, "mock");
        assert!(f.repos.audit.count().await.expect("统计审计失败") >= 1);
    }
}
