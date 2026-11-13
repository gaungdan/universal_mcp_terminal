//! 服务装配、越权与密文往返的集成测试。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use umt_app::AppState;
use umt_app::ipc::dto::{
    ConnectionFilterDto, CreateConnectionInput, CreateTokenInput, PageRequestDto, SecretInput,
    SetGrantInput,
};
use umt_app::services;
use umt_core::{Connection, ConnectionId, UnixMillis};
use umt_persistence::{ConnectionRepo, SecretBlob, SecretRepo};
use umt_security::{KeyStore, StaticKeyStore};
use uuid::Uuid;

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("umt-app-{}", Uuid::new_v4()))
}

fn new_keystore() -> Arc<dyn KeyStore> {
    Arc::new(StaticKeyStore::new())
}

fn ssh_connection_input(name: &str) -> CreateConnectionInput {
    CreateConnectionInput {
        name: name.to_string(),
        protocol_id: "ssh".into(),
        description: Some("测试连接".into()),
        tags: vec!["ops".into()],
        notes: None,
        enabled: true,
        config: json!({
            "host": "127.0.0.1",
            "port": 22,
            "username": "root",
            "auth_type": "password"
        }),
        secrets: vec![SecretInput {
            key: "password".into(),
            value: "s3cret-value".into(),
        }],
    }
}

#[tokio::test]
async fn services_survive_restart_and_secrets_roundtrip() {
    let dir = temp_dir();
    let keystore = new_keystore();
    let state = AppState::initialize_with_keystore(&dir, keystore.clone())
        .await
        .expect("初始化应用状态失败");

    let created = services::connections::create(&state, ssh_connection_input("prod"))
        .await
        .expect("创建连接失败");
    assert!(created.has_secret);
    assert_eq!(created.protocol_id, "ssh");

    let detail = services::connections::get(&state, &created.id)
        .await
        .expect("读取连接失败");
    assert_eq!(detail.secret_fields_set, vec!["password".to_string()]);
    assert_eq!(detail.config["host"], "127.0.0.1");
    assert!(
        detail.config.get("password").is_none(),
        "敏感字段不得出现在配置回显中"
    );

    let connection_id: ConnectionId = created.id.parse().expect("连接 ID 非法");
    let blob = state
        .repos
        .secrets
        .get(connection_id)
        .await
        .expect("读取密文失败")
        .expect("密文应已写入");
    assert_eq!(blob.nonce.len(), 12, "nonce 必须为 12 字节");
    assert!(!blob.ciphertext.is_empty());

    let decrypted = state.load_secrets(connection_id).await.expect("解密失败");
    assert_eq!(
        decrypted.get("password").map(String::as_str),
        Some("s3cret-value")
    );

    // MCP 注入的连接配置来源必须能读到同一份非敏感配置。
    let mcp_config = state
        .mcp_deps
        .configs
        .get_config(connection_id)
        .await
        .expect("MCP 配置读取失败")
        .expect("MCP 配置不应为空");
    assert_eq!(mcp_config.get_str("host"), Some("127.0.0.1"));

    let token = services::tokens::create(
        &state,
        CreateTokenInput {
            name: "claude".into(),
            description: None,
            expires_at: None,
        },
    )
    .await
    .expect("创建 Token 失败");
    assert!(token.plaintext.starts_with("umt_"));

    services::tokens::set_grant(
        &state,
        SetGrantInput {
            token_id: token.token.id.clone(),
            connection_id: created.id.clone(),
            allowed: true,
        },
    )
    .await
    .expect("授权失败");

    let context = state
        .access
        .authenticate(&token.plaintext, Some("integration-test".into()))
        .await
        .expect("认证失败");
    state
        .access
        .authorize_connection(&context, connection_id)
        .await
        .expect("授权后应允许访问");

    drop(state);

    // 重启：重建全部服务，数据必须仍然存在。
    let state = AppState::initialize_with_keystore(&dir, keystore)
        .await
        .expect("重启后初始化失败");

    let detail = services::connections::get(&state, &created.id)
        .await
        .expect("重启后读取连接失败");
    assert_eq!(detail.connection.name, "prod");
    assert_eq!(detail.secret_fields_set, vec!["password".to_string()]);

    let page = services::tokens::list(
        &state,
        PageRequestDto {
            offset: 0,
            limit: 50,
        },
    )
    .await
    .expect("重启后读取 Token 失败");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, token.token.id);

    let grants = services::tokens::list_grants(&state, &token.token.id)
        .await
        .expect("重启后读取授权失败");
    assert!(
        grants
            .iter()
            .any(|grant| grant.connection.id == created.id && grant.allowed),
        "重启后授权关系应保留"
    );

    let list = services::connections::list(
        &state,
        ConnectionFilterDto {
            protocol_id: None,
            enabled: Some(true),
            search: None,
            page: PageRequestDto {
                offset: 0,
                limit: 50,
            },
        },
    )
    .await
    .expect("重启后读取连接列表失败");
    assert_eq!(list.total, 1);

    let decrypted = state
        .load_secrets(connection_id)
        .await
        .expect("重启后解密失败");
    assert_eq!(
        decrypted.get("password").map(String::as_str),
        Some("s3cret-value")
    );

    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unauthorized_connection_is_rejected() {
    let dir = temp_dir();
    let state = AppState::initialize_with_keystore(&dir, new_keystore())
        .await
        .expect("初始化应用状态失败");

    let created = services::connections::create(&state, ssh_connection_input("denied"))
        .await
        .expect("创建连接失败");
    let connection_id: ConnectionId = created.id.parse().expect("连接 ID 非法");

    let token = services::tokens::create(
        &state,
        CreateTokenInput {
            name: "claude".into(),
            description: None,
            expires_at: None,
        },
    )
    .await
    .expect("创建 Token 失败");
    let context = state
        .access
        .authenticate(&token.plaintext, None)
        .await
        .expect("认证失败");

    let error = state
        .access
        .authorize_connection(&context, connection_id)
        .await
        .expect_err("未授权连接必须被拒绝");
    assert_eq!(error.code(), "access_denied");

    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn secret_cipher_roundtrip_and_nonce_validation() {
    let dir = temp_dir();
    let state = AppState::initialize_with_keystore(&dir, new_keystore())
        .await
        .expect("初始化应用状态失败");

    let connection_id = ConnectionId::new();
    let now = UnixMillis::now();
    state
        .repos
        .connections
        .create(Connection {
            id: connection_id,
            name: "cipher".into(),
            protocol_id: "ssh".into(),
            description: None,
            tags: vec![],
            notes: None,
            enabled: true,
            created_at: now,
            updated_at: now,
            revision: 1,
        })
        .await
        .expect("写入连接失败");

    let mut secrets = BTreeMap::new();
    secrets.insert("password".to_string(), "p@ssw0rd".to_string());
    state
        .store_secrets(connection_id, &secrets)
        .await
        .expect("加密写入失败");

    let blob = state
        .repos
        .secrets
        .get(connection_id)
        .await
        .expect("读取密文失败")
        .expect("密文应存在");
    assert_eq!(blob.nonce.len(), 12);
    assert_ne!(blob.ciphertext, b"p@ssw0rd".to_vec());

    let restored = state.load_secrets(connection_id).await.expect("解密失败");
    assert_eq!(restored, secrets);

    // nonce 长度不是 12 时必须报错而不是 panic。
    state
        .repos
        .secrets
        .put(
            connection_id,
            SecretBlob {
                ciphertext: blob.ciphertext,
                nonce: vec![0u8; 11],
                key_version: blob.key_version,
            },
        )
        .await
        .expect("写入损坏密文失败");
    let error = state
        .load_secrets(connection_id)
        .await
        .expect_err("非法 nonce 长度必须报错");
    assert_eq!(error.code(), "storage");

    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}
