//! SSH Host Key 信任管理。
//!
//! 设计约束：
//! - `endpoint` 一律由后端根据连接配置（host:port）推导，前端只提交连接 ID 与指纹；
//! - 信任动作写审计，便于追溯"谁在什么时候信任了哪个指纹"；
//! - 已信任指纹变化时，Provider 会返回 `HostKeyMismatch`，必须由用户显式重新信任。

use umt_core::{AuditKind, AuditOutcome, ExecutionState, UmtError, UnixMillis};
use umt_persistence::{HostKeyRecord, HostKeyRepo};

use crate::ipc::dto::{HostKeyDto, TrustHostKeyInput};
use crate::services::parse_id;
use crate::services::state::AppState;
use umt_core::ConnectionId;

fn to_dto(record: HostKeyRecord) -> HostKeyDto {
    HostKeyDto {
        endpoint: record.endpoint,
        host: record.host,
        port: record.port,
        algorithm: record.algorithm,
        fingerprint: record.fingerprint,
        trusted_at: record.trusted_at,
        trusted_by: record.trusted_by,
    }
}

pub async fn list(state: &AppState) -> Result<Vec<HostKeyDto>, UmtError> {
    Ok(state
        .repos
        .host_keys
        .list()
        .await?
        .into_iter()
        .map(to_dto)
        .collect())
}

/// 信任并保存连接当前的 Host Key 指纹。
pub async fn trust(state: &AppState, input: TrustHostKeyInput) -> Result<HostKeyDto, UmtError> {
    let connection_id: ConnectionId = parse_id(&input.connection_id, "连接")?;
    let fingerprint = input.fingerprint.trim();
    if !fingerprint.starts_with("SHA256:") || fingerprint.len() <= "SHA256:".len() {
        return Err(UmtError::Validation(
            "指纹格式非法，应形如 SHA256:xxxx".into(),
        ));
    }

    let config = state
        .configs
        .get(connection_id)
        .await?
        .ok_or_else(|| UmtError::NotFound {
            entity: "连接",
            id: input.connection_id.clone(),
        })?;
    let (Some(host), Some(port)) = (config.get_str("host"), config.get_u16("port")) else {
        return Err(UmtError::Validation(
            "该连接缺少 host/port，无法固定 Host Key 指纹".into(),
        ));
    };

    let record = HostKeyRecord {
        endpoint: format!("{host}:{port}"),
        host: host.to_string(),
        port,
        algorithm: input.algorithm.clone(),
        fingerprint: fingerprint.to_string(),
        trusted_at: UnixMillis::now(),
        trusted_by: Some("desktop".into()),
    };
    state.repos.host_keys.upsert(record.clone()).await?;

    state
        .audit
        .record(umt_persistence::NewAuditEntry {
            session_id: None,
            token_id: None,
            connection_id: Some(connection_id),
            kind: AuditKind::ConnectionUpdated,
            outcome: AuditOutcome::Allowed,
            command: Some(format!("host_key_trusted {}", record.endpoint)),
            working_dir: None,
            started_at: UnixMillis::now(),
            duration_ms: None,
            state: ExecutionState::Succeeded,
            exit_code: None,
            stdout_excerpt: Some(record.fingerprint.clone()),
            stderr_excerpt: None,
            error: None,
            redacted: false,
        })
        .await?;

    Ok(to_dto(record))
}

/// 撤销某个 endpoint 的信任，返回是否真的删除了记录。
pub async fn delete(state: &AppState, endpoint: &str) -> Result<bool, UmtError> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(UmtError::Validation("endpoint 不能为空".into()));
    }
    let deleted = state.repos.host_keys.delete(endpoint).await?;
    if deleted {
        state
            .audit
            .record(umt_persistence::NewAuditEntry {
                session_id: None,
                token_id: None,
                connection_id: None,
                kind: AuditKind::ConnectionUpdated,
                outcome: AuditOutcome::Allowed,
                command: Some(format!("host_key_revoked {endpoint}")),
                working_dir: None,
                started_at: UnixMillis::now(),
                duration_ms: None,
                state: ExecutionState::Succeeded,
                exit_code: None,
                stdout_excerpt: None,
                stderr_excerpt: None,
                error: None,
                redacted: false,
            })
            .await?;
    }
    Ok(deleted)
}
