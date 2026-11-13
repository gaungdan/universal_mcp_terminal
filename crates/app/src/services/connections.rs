//! 连接管理服务：校验、加密敏感字段、真实连接测试与复制。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use umt_core::{
    AuditKind, AuditOutcome, Connection, ConnectionConfig, ConnectionId, UmtError, UnixMillis,
};
use umt_persistence::{ConnectionFilter, ConnectionRepo};
use umt_protocol_api::{ConnectionTestResult, ProtocolProvider};

use crate::ipc::dto::{
    ConnectionDetailDto, ConnectionDto, ConnectionFilterDto, CreateConnectionInput,
    DraftConnectionTestInput, PageDto, SecretInput, UpdateConnectionInput,
};
use crate::services::audit::record_event;
use crate::services::state::AppState;

/// 连接仓库中的通用 DTO 映射。
pub async fn to_dto(
    state: &AppState,
    connection: &Connection,
    has_secret: bool,
) -> Result<ConnectionDto, UmtError> {
    let provider = state.protocols.get(&connection.protocol_id);
    let (protocol_display_name, capabilities) = match provider {
        Some(provider) => (
            provider.descriptor().display_name.clone(),
            provider
                .descriptor()
                .capabilities
                .as_strings()
                .into_iter()
                .map(str::to_string)
                .collect(),
        ),
        None => (connection.protocol_id.clone(), Vec::new()),
    };

    Ok(ConnectionDto {
        id: connection.id.to_string(),
        name: connection.name.clone(),
        protocol_id: connection.protocol_id.clone(),
        protocol_display_name,
        description: connection.description.clone(),
        tags: connection.tags.clone(),
        notes: connection.notes.clone(),
        enabled: connection.enabled,
        capabilities,
        has_secret,
        created_at: connection.created_at,
        updated_at: connection.updated_at,
        revision: connection.revision,
    })
}

pub async fn list(
    state: &AppState,
    filter: ConnectionFilterDto,
) -> Result<PageDto<ConnectionDto>, UmtError> {
    let page = state
        .repos
        .connections
        .list(&ConnectionFilter {
            protocol_id: filter.protocol_id,
            enabled: filter.enabled,
            search: filter.search,
            page: filter.page.into(),
        })
        .await?;

    let mut items = Vec::with_capacity(page.items.len());
    for connection in &page.items {
        let has_secret = state.has_secret(connection.id).await?;
        items.push(to_dto(state, connection, has_secret).await?);
    }
    Ok(PageDto {
        items,
        total: page.total,
    })
}

pub async fn get(state: &AppState, id: &str) -> Result<ConnectionDetailDto, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(id, "连接")?;
    let connection = require_connection(state, connection_id).await?;

    let config = state
        .configs
        .get(connection_id)
        .await?
        .map(|config| config.values)
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let secret_fields_set: Vec<String> = state
        .load_secrets(connection_id)
        .await?
        .keys()
        .cloned()
        .collect();
    let has_secret = !secret_fields_set.is_empty();

    Ok(ConnectionDetailDto {
        connection: to_dto(state, &connection, has_secret).await?,
        config,
        secret_fields_set,
    })
}

pub async fn create(
    state: &AppState,
    input: CreateConnectionInput,
) -> Result<ConnectionDto, UmtError> {
    let name = normalize_name(&input.name)?;
    let provider = state.protocols.require(&input.protocol_id)?;
    let (values, secrets) = split_config(provider.as_ref(), &input.config, &input.secrets)?;
    provider
        .validate_config(&ConnectionConfig::new(
            input.protocol_id.clone(),
            values.clone(),
        ))
        .await?;

    let now = UnixMillis::now();
    let connection = Connection {
        id: ConnectionId::new(),
        name,
        protocol_id: input.protocol_id,
        description: normalize_optional(input.description),
        tags: normalize_tags(input.tags),
        notes: normalize_optional(input.notes),
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
        revision: 1,
    };

    state.repos.connections.create(connection.clone()).await?;
    if let Err(err) = state.configs.set(connection.id, &values).await {
        let _ = state.repos.connections.delete(connection.id).await;
        return Err(err);
    }
    if let Err(err) = state.store_secrets(connection.id, &secrets).await {
        let _ = state.repos.connections.delete(connection.id).await;
        return Err(err);
    }

    record_event(
        state.audit.as_ref(),
        AuditKind::ConnectionCreated,
        AuditOutcome::Allowed,
        None,
        Some(connection.id),
        None,
        None,
    )
    .await;

    to_dto(state, &connection, !secrets.is_empty()).await
}

pub async fn update(
    state: &AppState,
    input: UpdateConnectionInput,
) -> Result<ConnectionDto, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(&input.id, "连接")?;
    let current = require_connection(state, connection_id).await?;
    let name = normalize_name(&input.name)?;
    let provider = state.protocols.require(&current.protocol_id)?;
    let (values, config_secrets) = split_config(provider.as_ref(), &input.config, &[])?;
    provider
        .validate_config(&ConnectionConfig::new(
            current.protocol_id.clone(),
            values.clone(),
        ))
        .await?;

    // `None` 保留已有密文；`Some` 整体替换（空列表等于清除）。
    let secrets = match input.secrets {
        Some(list) => Some(collect_secrets(provider.as_ref(), &list)?),
        None => None,
    };
    if !config_secrets.is_empty() && secrets.is_none() {
        return Err(UmtError::Validation("敏感字段必须通过 secrets 传入".into()));
    }

    let connection = Connection {
        id: current.id,
        name,
        protocol_id: current.protocol_id,
        description: normalize_optional(input.description),
        tags: normalize_tags(input.tags),
        notes: normalize_optional(input.notes),
        enabled: input.enabled,
        created_at: current.created_at,
        updated_at: UnixMillis::now(),
        revision: current.revision,
    };
    let updated = state
        .repos
        .connections
        .update(connection, input.expected_revision)
        .await?;
    state.configs.set(connection_id, &values).await?;
    if let Some(secrets) = secrets {
        state.store_secrets(connection_id, &secrets).await?;
    }

    record_event(
        state.audit.as_ref(),
        AuditKind::ConnectionUpdated,
        AuditOutcome::Allowed,
        None,
        Some(connection_id),
        None,
        None,
    )
    .await;

    let has_secret = state.has_secret(connection_id).await?;
    to_dto(state, &updated, has_secret).await
}

pub async fn delete(state: &AppState, id: &str) -> Result<bool, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(id, "连接")?;
    let deleted = state.repos.connections.delete(connection_id).await?;
    if deleted {
        record_event(
            state.audit.as_ref(),
            AuditKind::ConnectionDeleted,
            AuditOutcome::Allowed,
            None,
            Some(connection_id),
            None,
            None,
        )
        .await;
    }
    Ok(deleted)
}

pub async fn set_enabled(
    state: &AppState,
    id: &str,
    enabled: bool,
) -> Result<ConnectionDto, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(id, "连接")?;
    state
        .repos
        .connections
        .set_enabled(connection_id, enabled)
        .await?;
    let connection = require_connection(state, connection_id).await?;
    let has_secret = state.has_secret(connection_id).await?;
    to_dto(state, &connection, has_secret).await
}

pub async fn duplicate(state: &AppState, id: &str) -> Result<ConnectionDto, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(id, "连接")?;
    let source = require_connection(state, connection_id).await?;
    let values = state
        .configs
        .get(connection_id)
        .await?
        .map(|config| config.values)
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    let name = unique_name(state, &source.name).await?;
    let now = UnixMillis::now();
    let connection = Connection {
        id: ConnectionId::new(),
        name,
        protocol_id: source.protocol_id,
        description: source.description,
        tags: source.tags,
        notes: source.notes,
        enabled: source.enabled,
        created_at: now,
        updated_at: now,
        revision: 1,
    };

    state.repos.connections.create(connection.clone()).await?;
    if let Err(err) = state.configs.set(connection.id, &values).await {
        let _ = state.repos.connections.delete(connection.id).await;
        return Err(err);
    }
    // 安全约束：复制不携带任何密文，副本需要重新填写敏感字段。

    record_event(
        state.audit.as_ref(),
        AuditKind::ConnectionCreated,
        AuditOutcome::Allowed,
        None,
        Some(connection.id),
        None,
        None,
    )
    .await;

    to_dto(state, &connection, false).await
}

pub async fn test(state: &AppState, id: &str) -> Result<ConnectionTestResult, UmtError> {
    let connection_id = crate::services::parse_id::<ConnectionId>(id, "连接")?;
    let connection = require_connection(state, connection_id).await?;
    let config = state
        .configs
        .get(connection_id)
        .await?
        .ok_or_else(|| UmtError::NotFound {
            entity: "连接配置",
            id: connection_id.to_string(),
        })?;
    let secrets = state.load_secrets(connection_id).await?;
    let provider = state.protocols.require(&connection.protocol_id)?;
    let merged = merge_secrets(&config.values, &secrets);
    let result = provider
        .test_connection(&ConnectionConfig::new(
            connection.protocol_id.clone(),
            merged,
        ))
        .await?;

    record_event(
        state.audit.as_ref(),
        AuditKind::ConnectionTested,
        if result.ok {
            AuditOutcome::Allowed
        } else {
            AuditOutcome::Failed
        },
        None,
        Some(connection_id),
        None,
        None,
    )
    .await;

    Ok(result)
}

pub async fn test_draft(
    state: &AppState,
    input: DraftConnectionTestInput,
) -> Result<ConnectionTestResult, UmtError> {
    let provider = state.protocols.require(&input.protocol_id)?;
    let (values, secrets) = split_config(provider.as_ref(), &input.config, &input.secrets)?;
    let merged = merge_secrets(&values, &secrets);
    let result = provider
        .test_connection(&ConnectionConfig::new(input.protocol_id, merged))
        .await?;

    record_event(
        state.audit.as_ref(),
        AuditKind::ConnectionTested,
        if result.ok {
            AuditOutcome::Allowed
        } else {
            AuditOutcome::Failed
        },
        None,
        None,
        None,
        None,
    )
    .await;

    Ok(result)
}

async fn require_connection(
    state: &AppState,
    connection_id: ConnectionId,
) -> Result<Connection, UmtError> {
    state
        .repos
        .connections
        .get(connection_id)
        .await?
        .ok_or_else(|| UmtError::NotFound {
            entity: "连接",
            id: connection_id.to_string(),
        })
}

async fn unique_name(state: &AppState, base: &str) -> Result<String, UmtError> {
    let base = base.trim();
    for attempt in 0..100u32 {
        let candidate = match attempt {
            0 => format!("{base} 副本"),
            n => format!("{base} 副本 {}", n + 1),
        };
        let existing = state
            .repos
            .connections
            .list(&ConnectionFilter {
                search: Some(candidate.clone()),
                page: umt_core::PageRequest::new(0, 500),
                ..Default::default()
            })
            .await?;
        if !existing
            .items
            .iter()
            .any(|connection| connection.name == candidate)
        {
            return Ok(candidate);
        }
    }
    Err(UmtError::Conflict("无法生成不重复的副本名称".into()))
}

/// 拆分配置与敏感字段：Schema 中 `secret = true` 的字段永远不进入 `config_json`。
fn split_config(
    provider: &dyn ProtocolProvider,
    config: &Value,
    extra: &[SecretInput],
) -> Result<(Value, BTreeMap<String, String>), UmtError> {
    let mut object = match config {
        Value::Object(map) => map.clone(),
        Value::Null => serde_json::Map::new(),
        _ => return Err(UmtError::Validation("连接配置必须是 JSON 对象".into())),
    };

    let mut secrets = BTreeMap::new();
    let mut secret_keys = BTreeSet::new();
    for field in provider.config_schema().fields.iter().filter(|f| f.secret) {
        secret_keys.insert(field.key.as_str());
        if let Some(value) = object.remove(&field.key)
            && let Some(text) = value.as_str().filter(|text| !text.is_empty())
        {
            secrets.insert(field.key.clone(), text.to_string());
        }
    }

    for (key, value) in collect_secrets(provider, extra)? {
        secrets.insert(key, value);
    }

    Ok((Value::Object(object), secrets))
}

/// 只接受 Schema 声明为敏感字段的键，防止任意内容被塞进密文。
fn collect_secrets(
    provider: &dyn ProtocolProvider,
    list: &[SecretInput],
) -> Result<BTreeMap<String, String>, UmtError> {
    let allowed: BTreeSet<&str> = provider
        .config_schema()
        .fields
        .iter()
        .filter(|field| field.secret)
        .map(|field| field.key.as_str())
        .collect();

    let mut secrets = BTreeMap::new();
    for item in list {
        let key = item.key.trim();
        if key.is_empty() || item.value.is_empty() {
            continue;
        }
        if !allowed.contains(key) {
            return Err(UmtError::Validation(format!(
                "字段 {key} 不是该协议声明的敏感字段"
            )));
        }
        secrets.insert(key.to_string(), item.value.clone());
    }
    Ok(secrets)
}

fn merge_secrets(config: &Value, secrets: &BTreeMap<String, String>) -> Value {
    let mut object = config.as_object().cloned().unwrap_or_default();
    for (key, value) in secrets {
        object.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(object)
}

fn normalize_name(name: &str) -> Result<String, UmtError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(UmtError::Validation("连接名称不能为空".into()));
    }
    Ok(name.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut result = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !result.iter().any(|existing| existing == tag) {
            result.push(tag.to_string());
        }
    }
    result
}
