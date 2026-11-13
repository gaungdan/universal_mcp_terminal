//! MCP Server 生命周期：启动、停止、状态查询与端口占用检测。

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use umt_core::UmtError;

use crate::deps::McpDeps;
use crate::handler::UmtMcpServer;

/// 服务监听配置。默认只监听回环地址，避免误暴露到局域网。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 17321,
        }
    }
}

/// 运行状态快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub last_error: Option<String>,
}

struct ServerState {
    running: bool,
    port: u16,
    endpoint: String,
    last_error: Option<String>,
    cancellation: Option<CancellationToken>,
    task: Option<JoinHandle<()>>,
}

/// MCP 服务句柄。`start` / `stop` 可重复调用。
pub struct McpServer {
    config: McpServerConfig,
    deps: Arc<McpDeps>,
    state: Arc<Mutex<ServerState>>,
}

impl McpServer {
    pub fn new(config: McpServerConfig, deps: impl Into<Arc<McpDeps>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState {
                running: false,
                port: config.port,
                endpoint: String::new(),
                last_error: None,
                cancellation: None,
                task: None,
            })),
            config,
            deps: deps.into(),
        }
    }

    /// 启动服务，返回 MCP endpoint。重复调用直接返回当前 endpoint。
    ///
    /// 端口被占用时返回 `UmtError::Conflict`，并在 `status().last_error` 中保留原因。
    pub async fn start(&self) -> Result<String, UmtError> {
        {
            let state = self.lock();
            if state.running {
                return Ok(state.endpoint.clone());
            }
        }

        let address = format!("{}:{}", self.config.host, self.config.port);
        let listener = match TcpListener::bind(&address).await {
            Ok(listener) => listener,
            Err(err) => {
                let message = format!("MCP 服务无法监听 {address}：{err}（端口可能已被占用）");
                tracing::error!(address = %address, error = %err, "MCP 服务监听失败");
                self.set_last_error(message.clone());
                return Err(UmtError::Conflict(message));
            }
        };

        // port = 0 时由系统分配，endpoint 必须使用真实端口。
        let port = listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(self.config.port);
        let endpoint = format!("http://{}:{port}/mcp", self.config.host);

        let cancellation = CancellationToken::new();
        let transport_config = StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation.child_token());
        let server = UmtMcpServer::new(self.deps.clone());
        let http_service: StreamableHttpService<UmtMcpServer, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(server.clone()),
                Arc::new(LocalSessionManager::default()),
                transport_config,
            );
        let access = self.deps.access.clone();
        let audit = self.deps.audit.clone();
        let router = Router::new()
            .nest_service("/mcp", http_service)
            .layer(middleware::from_fn(
                move |request: Request<Body>, next: Next| {
                    let access = access.clone();
                    let audit = audit.clone();
                    async move {
                        let bearer = request
                            .headers()
                            .get(AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.strip_prefix("Bearer "))
                            .map(str::trim)
                            .filter(|token| !token.is_empty());

                        let Some(bearer) = bearer else {
                            record_denied(&audit, "缺少 Bearer Token").await;
                            return unauthorized("缺少 Bearer Token");
                        };
                        match access.authenticate(bearer, None).await {
                            Ok(_context) => next.run(request).await,
                            Err(error) => {
                                record_denied(&audit, &error.to_string()).await;
                                unauthorized(&error.to_string())
                            }
                        }
                    }
                },
            ));

        let shutdown = cancellation.clone();
        let shared_state = self.state.clone();
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await;
            let mut state = lock(&shared_state);
            state.running = false;
            state.endpoint.clear();
            if let Err(err) = result {
                tracing::error!(error = %err, "MCP 服务异常退出");
                state.last_error = Some(format!("MCP 服务异常退出：{err}"));
            }
        });

        let mut state = self.lock();
        state.running = true;
        state.port = port;
        state.endpoint = endpoint.clone();
        state.last_error = None;
        state.cancellation = Some(cancellation);
        state.task = Some(task);
        tracing::info!(endpoint = %endpoint, "MCP 服务已启动");
        Ok(endpoint)
    }

    /// 通过 CancellationToken 优雅关闭，并等待服务任务退出（最多 5 秒）。
    pub async fn stop(&self) -> Result<(), UmtError> {
        let (cancellation, task) = {
            let mut state = self.lock();
            if !state.running && state.task.is_none() {
                return Ok(());
            }
            state.running = false;
            state.endpoint.clear();
            (state.cancellation.take(), state.task.take())
        };

        if let Some(token) = cancellation {
            token.cancel();
        }
        if let Some(handle) = task {
            match tokio::time::timeout(Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::error!(error = %err, "MCP 服务任务异常结束");
                    self.set_last_error(format!("MCP 服务任务异常结束：{err}"));
                }
                Err(_) => tracing::warn!("MCP 服务停止超时，已放弃等待"),
            }
        }
        tracing::info!("MCP 服务已停止");
        Ok(())
    }

    /// 当前运行状态。
    pub fn status(&self) -> McpStatus {
        let state = self.lock();
        McpStatus {
            running: state.running,
            host: self.config.host.clone(),
            port: state.port,
            endpoint: state.endpoint.clone(),
            last_error: state.last_error.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ServerState> {
        lock(&self.state)
    }

    fn set_last_error(&self, message: String) {
        let mut state = self.lock();
        state.running = false;
        state.last_error = Some(message);
    }
}

fn lock(state: &Mutex<ServerState>) -> MutexGuard<'_, ServerState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// 记录鉴权失败的审计。失败只记日志，不能影响 401 响应。
async fn record_denied(audit: &umt_audit::AuditService, message: &str) {
    use umt_core::{AuditKind, AuditOutcome, ExecutionState, UnixMillis};
    let entry = umt_persistence::NewAuditEntry {
        session_id: None,
        token_id: None,
        connection_id: None,
        kind: AuditKind::PermissionDenied,
        outcome: AuditOutcome::Denied,
        command: Some("mcp_http.authenticate".to_string()),
        working_dir: None,
        started_at: UnixMillis::now(),
        duration_ms: None,
        state: ExecutionState::Failed,
        exit_code: None,
        stdout_excerpt: None,
        stderr_excerpt: None,
        error: Some(message.to_string()),
        redacted: false,
    };
    if let Err(err) = audit.record(entry).await {
        tracing::warn!(error = %err, "写入鉴权失败审计失败");
    }
}

/// 未认证响应。MCP 协议层之外直接返回 401，避免客户端误以为握手成功。
fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "message": message,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestEnv;

    #[tokio::test]
    async fn start_binds_ephemeral_port_and_stop_shuts_down() {
        let env = TestEnv::new();
        let server = McpServer::new(
            McpServerConfig {
                host: "127.0.0.1".into(),
                port: 0,
            },
            env.deps.clone(),
        );

        let endpoint = server.start().await.expect("启动失败");
        assert!(endpoint.starts_with("http://127.0.0.1:"), "{endpoint}");
        assert!(endpoint.ends_with("/mcp"), "{endpoint}");

        let status = server.status();
        assert!(status.running);
        assert!(status.port > 0);
        assert_eq!(status.endpoint, endpoint);
        assert!(status.last_error.is_none());

        server.stop().await.expect("停止失败");
        let status = server.status();
        assert!(!status.running);
        assert!(status.endpoint.is_empty());
    }

    #[tokio::test]
    async fn occupied_port_returns_explicit_error() {
        let blocker = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("绑定占位端口失败");
        let port = blocker.local_addr().expect("读取端口失败").port();

        let env = TestEnv::new();
        let server = McpServer::new(
            McpServerConfig {
                host: "127.0.0.1".into(),
                port,
            },
            env.deps.clone(),
        );

        let error = server.start().await.expect_err("端口占用必须报错");
        assert!(error.to_string().contains("端口"), "实际错误：{error}");

        let status = server.status();
        assert!(!status.running);
        assert!(status.last_error.is_some());
        drop(blocker);
    }
}
