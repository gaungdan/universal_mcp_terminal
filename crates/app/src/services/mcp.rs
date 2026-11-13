//! MCP Server 生命周期与监听配置。

use umt_core::{AuditKind, AuditOutcome, UmtError};
use umt_mcp_server::{McpServer, McpServerConfig, McpStatus};

use crate::ipc::dto::{McpConfigInput, McpStatusDto};
use crate::services::audit::record_event;
use crate::services::state::{AppState, KEY_MCP_HOST, KEY_MCP_PORT};

pub async fn status(state: &AppState) -> McpStatusDto {
    let server = state.mcp.lock().await;
    to_dto(server.status())
}

pub async fn start(
    state: &AppState,
    config: Option<McpConfigInput>,
) -> Result<McpStatusDto, UmtError> {
    if let Some(input) = config {
        let input = normalize(&input)?;
        persist(state, &input).await?;
        rebuild(state, &input).await?;
    }

    let server = state.mcp.lock().await;
    let result = server.start().await;
    let status = to_dto(server.status());
    drop(server);
    result?;

    record_event(
        state.audit.as_ref(),
        AuditKind::McpServerStarted,
        AuditOutcome::Allowed,
        None,
        None,
        None,
        None,
    )
    .await;
    Ok(status)
}

pub async fn stop(state: &AppState) -> Result<McpStatusDto, UmtError> {
    let server = state.mcp.lock().await;
    server.stop().await?;
    let status = to_dto(server.status());
    drop(server);

    record_event(
        state.audit.as_ref(),
        AuditKind::McpServerStopped,
        AuditOutcome::Allowed,
        None,
        None,
        None,
        None,
    )
    .await;
    Ok(status)
}

/// 保存监听配置；服务正在运行时按新配置重启，否则只替换待启动的句柄。
pub async fn update_settings(
    state: &AppState,
    input: McpConfigInput,
) -> Result<McpStatusDto, UmtError> {
    let input = normalize(&input)?;
    let running = {
        let server = state.mcp.lock().await;
        server.status().running
    };

    persist(state, &input).await?;
    let replaced = rebuild(state, &input).await?;
    if replaced && running {
        let server = state.mcp.lock().await;
        let result = server.start().await;
        let status = to_dto(server.status());
        drop(server);
        result?;
        record_event(
            state.audit.as_ref(),
            AuditKind::McpServerStarted,
            AuditOutcome::Allowed,
            None,
            None,
            None,
            None,
        )
        .await;
        return Ok(status);
    }

    Ok(status(state).await)
}

fn normalize(input: &McpConfigInput) -> Result<McpConfigInput, UmtError> {
    let host = input.host.trim();
    if host.is_empty() {
        return Err(UmtError::Validation("监听地址不能为空".into()));
    }
    if input.port == 0 {
        return Err(UmtError::Validation("端口必须在 1-65535 之间".into()));
    }
    Ok(McpConfigInput {
        host: host.to_string(),
        port: input.port,
    })
}

async fn persist(state: &AppState, input: &McpConfigInput) -> Result<(), UmtError> {
    state.set_setting(KEY_MCP_HOST, &input.host).await?;
    state
        .set_setting(KEY_MCP_PORT, &input.port.to_string())
        .await?;
    Ok(())
}

/// 监听参数变化时替换句柄；返回是否发生了替换。
async fn rebuild(state: &AppState, input: &McpConfigInput) -> Result<bool, UmtError> {
    let mut server = state.mcp.lock().await;
    let current = server.status();
    if current.host == input.host && current.port == input.port {
        return Ok(false);
    }

    server.stop().await?;
    *server = McpServer::new(
        McpServerConfig {
            host: input.host.clone(),
            port: input.port,
        },
        state.mcp_deps.clone(),
    );
    Ok(true)
}

fn to_dto(status: McpStatus) -> McpStatusDto {
    McpStatusDto {
        running: status.running,
        host: status.host,
        port: status.port,
        endpoint: status.endpoint,
        last_error: status.last_error,
    }
}
