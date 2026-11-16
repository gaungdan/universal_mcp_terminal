//! SQLite 仓库的集成测试。
//!
//! 每个测试使用独立的临时数据库文件，结束后连同 `-wal` / `-shm` 一起删除。

use std::path::{Path, PathBuf};

use umt_core::{
    AuditId, AuditKind, AuditOutcome, AuditQuery, Connection, ConnectionId, ExecutionState,
    PageRequest, SessionId, SessionInfo, SessionState, Token, TokenId, TokenStatus, UnixMillis,
};

use super::SqliteRepos;
use crate::models::{ConnectionFilter, HostKeyRecord, NewAuditEntry, SecretBlob, SessionFilter};
use crate::repo::{
    AuditRepo, ConnectionRepo, GrantRepo, HostKeyRepo, SecretRepo, SessionRepo, SettingsRepo,
    TokenRepo,
};
use crate::{Database, PersistenceError};

fn db_path() -> PathBuf {
    std::env::temp_dir().join(format!("umt-sqlite-repo-{}.db", uuid::Uuid::new_v4()))
}

fn cleanup(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

async fn open(path: &Path) -> SqliteRepos {
    let db = Database::connect(path).await.expect("打开数据库应成功");
    SqliteRepos::new(db.pool().clone())
}

fn sample_connection(name: &str, protocol: &str, created_at: i64) -> Connection {
    Connection {
        id: ConnectionId::new(),
        name: name.to_string(),
        protocol_id: protocol.to_string(),
        description: Some(format!("{name} 描述")),
        tags: vec!["prod".to_string()],
        notes: None,
        enabled: true,
        created_at: UnixMillis(created_at),
        updated_at: UnixMillis(created_at),
        revision: 1,
    }
}

fn sample_token(name: &str, prefix: &str, created_at: i64) -> crate::TokenRecord {
    crate::TokenRecord {
        token: Token {
            id: TokenId::new(),
            name: name.to_string(),
            description: None,
            prefix: prefix.to_string(),
            status: TokenStatus::Active,
            permission: umt_core::TokenPermission::ReadOnly,
            allow_interactive: false,
            expires_at: None,
            created_at: UnixMillis(created_at),
            last_used_at: None,
        },
        hmac: format!("hmac-{prefix}"),
    }
}

fn sample_session(
    id: SessionId,
    token_id: TokenId,
    connection_id: ConnectionId,
    state: SessionState,
    started_at: i64,
) -> SessionInfo {
    SessionInfo {
        id,
        token_id,
        connection_id,
        protocol_id: "ssh".to_string(),
        client_info: Some("claude".to_string()),
        state,
        started_at: UnixMillis(started_at),
        ended_at: None,
        close_reason: None,
        rows: 40,
        cols: 120,
    }
}

fn sample_audit(
    kind: AuditKind,
    token_id: TokenId,
    connection_id: ConnectionId,
    started_at: i64,
) -> NewAuditEntry {
    NewAuditEntry {
        session_id: None,
        token_id: Some(token_id),
        connection_id: Some(connection_id),
        kind,
        outcome: AuditOutcome::Allowed,
        tool_name: None,
        permission: None,
        command: Some("ls".to_string()),
        working_dir: Some("/tmp".to_string()),
        started_at: UnixMillis(started_at),
        duration_ms: Some(5),
        state: ExecutionState::Succeeded,
        exit_code: Some(0),
        stdout_excerpt: None,
        stderr_excerpt: None,
        error: None,
        redacted: false,
    }
}

fn sample_host_key() -> HostKeyRecord {
    HostKeyRecord {
        endpoint: "10.0.0.5:22".to_string(),
        host: "10.0.0.5".to_string(),
        port: 22,
        algorithm: Some("ssh-ed25519".to_string()),
        fingerprint: "SHA256:abc".to_string(),
        trusted_at: UnixMillis(1_000),
        trusted_by: None,
    }
}

/// 本工单最重要的验收点：写入后关闭整个池，重新打开同一文件数据仍在。
#[tokio::test]
async fn data_survives_restart() {
    let path = db_path();
    let connection = sample_connection("restart", "ssh", 1_000);
    let record = sample_token("restart-token", "umt_restart", 2_000);
    let session = sample_session(
        SessionId::new(),
        record.token.id,
        connection.id,
        SessionState::Running,
        3_000,
    );
    let secret = SecretBlob {
        ciphertext: vec![1, 2, 3],
        nonce: vec![9, 8],
        key_version: 2,
    };
    let host_key = sample_host_key();

    let (conn_id, token_id, audit_id) = {
        let db = Database::connect(&path).await.expect("首次打开数据库");
        let repos = SqliteRepos::new(db.pool().clone());

        repos
            .connections
            .create(connection.clone())
            .await
            .expect("写入连接");
        repos
            .tokens
            .create(record.clone())
            .await
            .expect("写入 Token");
        repos
            .grants
            .set(record.token.id, connection.id, true)
            .await
            .expect("写入授权");
        repos.sessions.create(&session).await.expect("写入会话");
        repos
            .secrets
            .put(connection.id, secret.clone())
            .await
            .expect("写入密文");
        repos
            .settings
            .set("ui.theme", "dark")
            .await
            .expect("写入设置");
        repos
            .host_keys
            .upsert(host_key.clone())
            .await
            .expect("写入 Host Key");
        let audit_id = repos
            .audit
            .append(sample_audit(
                AuditKind::SessionOpened,
                record.token.id,
                connection.id,
                4_000,
            ))
            .await
            .expect("写入审计");

        drop(repos);
        drop(db);
        (connection.id, record.token.id, audit_id)
    };

    // 重新打开同一路径，模拟进程重启。
    let db = Database::connect(&path).await.expect("重新打开数据库");
    let repos = SqliteRepos::new(db.pool().clone());

    assert_eq!(
        repos.connections.get(conn_id).await.expect("读取连接"),
        Some(connection),
        "连接应持久化"
    );
    assert_eq!(
        repos.tokens.get_record(token_id).await.expect("读取 Token"),
        Some(record),
        "Token 与 HMAC 应持久化"
    );
    assert!(
        repos
            .grants
            .is_allowed(token_id, conn_id)
            .await
            .expect("读取授权"),
        "授权应持久化"
    );
    assert_eq!(
        repos.sessions.get(session.id).await.expect("读取会话"),
        Some(session),
        "会话应持久化"
    );
    assert_eq!(
        repos.secrets.get(conn_id).await.expect("读取密文"),
        Some(secret),
        "密文应持久化"
    );
    assert_eq!(
        repos.settings.get("ui.theme").await.expect("读取设置"),
        Some("dark".to_string())
    );
    assert_eq!(
        repos
            .host_keys
            .get("10.0.0.5:22")
            .await
            .expect("读取 Host Key"),
        Some(host_key)
    );
    let audit = repos
        .audit
        .get(audit_id)
        .await
        .expect("读取审计")
        .expect("审计应存在");
    assert_eq!(audit.id, audit_id);
    assert_eq!(audit.kind, AuditKind::SessionOpened);
    assert_eq!(repos.audit.count().await.expect("审计计数"), 1);

    drop(repos);
    drop(db);
    cleanup(&path);
}

#[tokio::test]
async fn connection_revision_and_unique_constraints() {
    let path = db_path();
    let repos = open(&path).await;

    let connection = sample_connection("web", "ssh", 1_000);
    let id = connection.id;
    repos
        .connections
        .create(connection.clone())
        .await
        .expect("创建应成功");
    assert_eq!(repos.connections.count().await.expect("计数"), 1);
    assert_eq!(
        repos.connections.get(id).await.expect("读取"),
        Some(connection.clone())
    );

    // 重复 ID 与重复名称的语义不同
    assert!(matches!(
        repos.connections.create(connection.clone()).await,
        Err(PersistenceError::Conflict(_))
    ));
    let mut duplicate_name = sample_connection("web", "telnet", 1_001);
    duplicate_name.id = ConnectionId::new();
    assert!(matches!(
        repos.connections.create(duplicate_name).await,
        Err(PersistenceError::UniqueViolation(_))
    ));

    // 版本不符
    let mut edited = connection.clone();
    edited.name = "web-2".to_string();
    assert!(matches!(
        repos.connections.update(edited.clone(), 99).await,
        Err(PersistenceError::Conflict(_))
    ));

    // 版本正确：revision 变为 2，并返回写入后的记录
    let updated = repos
        .connections
        .update(edited.clone(), 1)
        .await
        .expect("版本正确应成功");
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.name, "web-2");
    assert_eq!(
        repos
            .connections
            .get(id)
            .await
            .expect("读取")
            .expect("应存在")
            .revision,
        2
    );

    // 使用旧版本再次更新仍然冲突
    assert!(matches!(
        repos.connections.update(connection.clone(), 1).await,
        Err(PersistenceError::Conflict(_))
    ));

    // 记录不存在
    assert!(matches!(
        repos
            .connections
            .update(sample_connection("ghost", "ssh", 1_000), 1)
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    // set_enabled 同样推进 revision
    repos
        .connections
        .set_enabled(id, false)
        .await
        .expect("停用应成功");
    let after = repos
        .connections
        .get(id)
        .await
        .expect("读取")
        .expect("应存在");
    assert!(!after.enabled);
    assert_eq!(after.revision, 3);
    assert!(matches!(
        repos
            .connections
            .set_enabled(ConnectionId::new(), true)
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    assert!(repos.connections.delete(id).await.expect("删除应成功"));
    assert!(!repos.connections.delete(id).await.expect("再删返回 false"));
    assert_eq!(repos.connections.get(id).await.expect("读取"), None);

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn connection_list_filters_and_paginates() {
    let path = db_path();
    let repos = open(&path).await;

    repos
        .connections
        .create(sample_connection("alpha", "ssh", 1_000))
        .await
        .expect("创建");
    let beta = sample_connection("beta", "telnet", 2_000);
    let beta_id = beta.id;
    repos.connections.create(beta).await.expect("创建");
    repos
        .connections
        .create(sample_connection("gamma", "ssh", 3_000))
        .await
        .expect("创建");

    let ssh_only = ConnectionFilter {
        protocol_id: Some("ssh".to_string()),
        ..ConnectionFilter::default()
    };
    let page = repos.connections.list(&ssh_only).await.expect("查询");
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].name, "alpha");

    let by_search = ConnectionFilter {
        search: Some("BET".to_string()),
        ..ConnectionFilter::default()
    };
    let page = repos.connections.list(&by_search).await.expect("查询");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "beta");

    let by_tag = ConnectionFilter {
        search: Some("PROD".to_string()),
        ..ConnectionFilter::default()
    };
    assert_eq!(
        repos.connections.list(&by_tag).await.expect("查询").total,
        3
    );

    let paged = ConnectionFilter {
        page: PageRequest::new(1, 1),
        ..ConnectionFilter::default()
    };
    let page = repos.connections.list(&paged).await.expect("查询");
    assert_eq!(page.total, 3, "total 是过滤后的总数");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "beta");

    repos
        .connections
        .set_enabled(beta_id, false)
        .await
        .expect("停用");
    let disabled = ConnectionFilter {
        enabled: Some(false),
        ..ConnectionFilter::default()
    };
    let page = repos.connections.list(&disabled).await.expect("查询");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, beta_id);

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn token_lifecycle_and_metadata() {
    let path = db_path();
    let repos = open(&path).await;

    let record = sample_token("claude", "umt_aaaa", 1_000);
    let id = record.token.id;
    repos.tokens.create(record.clone()).await.expect("创建");

    // 重复 ID → Conflict；重复前缀 → UniqueViolation
    assert!(matches!(
        repos.tokens.create(record.clone()).await,
        Err(PersistenceError::Conflict(_))
    ));
    assert!(matches!(
        repos
            .tokens
            .create(sample_token("other", "umt_aaaa", 2_000))
            .await,
        Err(PersistenceError::UniqueViolation(_))
    ));

    let found = repos
        .tokens
        .find_by_prefix("umt_aaaa")
        .await
        .expect("查询")
        .expect("应命中");
    assert_eq!(found.token.id, id);
    assert_eq!(found.hmac, "hmac-umt_aaaa");
    assert!(
        repos
            .tokens
            .find_by_prefix("umt_zzzz")
            .await
            .expect("查询")
            .is_none()
    );

    // 元数据更新不影响状态
    repos
        .tokens
        .update_metadata(id, "claude-2", Some("新描述"), Some(UnixMillis(5_000)))
        .await
        .expect("更新元数据");
    let updated = repos.tokens.get(id).await.expect("读取").expect("应存在");
    assert_eq!(updated.name, "claude-2");
    assert_eq!(updated.description.as_deref(), Some("新描述"));
    assert_eq!(updated.expires_at, Some(UnixMillis(5_000)));
    assert_eq!(updated.status, TokenStatus::Active);
    assert_eq!(updated.prefix, "umt_aaaa");

    assert!(matches!(
        repos
            .tokens
            .update_metadata(TokenId::new(), "ghost", None, None)
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    repos
        .tokens
        .set_status(id, TokenStatus::Revoked)
        .await
        .expect("改状态");
    assert_eq!(
        repos
            .tokens
            .get(id)
            .await
            .expect("读取")
            .expect("应存在")
            .status,
        TokenStatus::Revoked
    );
    assert!(matches!(
        repos
            .tokens
            .set_status(TokenId::new(), TokenStatus::Disabled)
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    let now = UnixMillis(9_999);
    repos
        .tokens
        .touch_last_used(id, now)
        .await
        .expect("刷新最近使用");
    assert_eq!(
        repos
            .tokens
            .get(id)
            .await
            .expect("读取")
            .expect("应存在")
            .last_used_at,
        Some(now)
    );
    assert!(matches!(
        repos.tokens.touch_last_used(TokenId::new(), now).await,
        Err(PersistenceError::NotFound { .. })
    ));

    // 列表按创建时间倒序
    repos
        .tokens
        .create(sample_token("newer", "umt_bbbb", 2_000))
        .await
        .expect("创建");
    assert_eq!(repos.tokens.count().await.expect("计数"), 2);
    let page = repos
        .tokens
        .list(PageRequest::default())
        .await
        .expect("列表");
    assert_eq!(page.total, 2);
    assert_eq!(page.items[0].prefix, "umt_bbbb");

    assert!(repos.tokens.delete(id).await.expect("删除"));
    assert!(!repos.tokens.delete(id).await.expect("再删返回 false"));
    assert_eq!(repos.tokens.count().await.expect("计数"), 1);

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn grants_toggle_both_directions() {
    let path = db_path();
    let repos = open(&path).await;

    let c1 = sample_connection("c1", "ssh", 1_000);
    let c2 = sample_connection("c2", "ssh", 2_000);
    let c3 = sample_connection("c3", "ssh", 3_000);
    let (c1_id, c2_id, c3_id) = (c1.id, c2.id, c3.id);
    repos.connections.create(c1).await.expect("创建");
    repos.connections.create(c2).await.expect("创建");
    repos.connections.create(c3).await.expect("创建");

    let record = sample_token("t1", "umt_tttt", 1_000);
    let token_id = record.token.id;
    repos.tokens.create(record).await.expect("创建");

    repos.grants.set(token_id, c1_id, true).await.expect("授权");
    repos
        .grants
        .set(token_id, c2_id, false)
        .await
        .expect("显式拒绝");

    assert!(
        repos
            .grants
            .is_allowed(token_id, c1_id)
            .await
            .expect("查询")
    );
    assert!(
        !repos
            .grants
            .is_allowed(token_id, c2_id)
            .await
            .expect("查询")
    );

    // 正向：全部连接都要出现，未授权的 c3 为 false
    let views = repos.grants.list_for_token(token_id).await.expect("视图");
    assert_eq!(views.len(), 3, "应返回全部连接");
    assert_eq!(views[0].connection.id, c1_id);
    assert!(views[0].allowed);
    assert_eq!(views[1].connection.id, c2_id);
    assert!(!views[1].allowed);
    assert_eq!(views[2].connection.id, c3_id);
    assert!(!views[2].allowed, "未设置授权视为拒绝");

    // 反向：只有 allowed = 1 的 Token 出现
    assert_eq!(
        repos
            .grants
            .list_tokens_for_connection(c1_id)
            .await
            .expect("查询"),
        vec![token_id]
    );
    assert!(
        repos
            .grants
            .list_tokens_for_connection(c2_id)
            .await
            .expect("查询")
            .is_empty()
    );

    // 关闭开关后双向都不再生效
    repos
        .grants
        .set(token_id, c1_id, false)
        .await
        .expect("关闭");
    assert!(
        !repos
            .grants
            .is_allowed(token_id, c1_id)
            .await
            .expect("查询")
    );
    assert!(
        repos
            .grants
            .list_tokens_for_connection(c1_id)
            .await
            .expect("查询")
            .is_empty()
    );

    // 重新打开并按 Token 全量撤销（c1、c2 两行）
    repos.grants.set(token_id, c1_id, true).await.expect("授权");
    assert_eq!(
        repos
            .grants
            .revoke_all_for_token(token_id)
            .await
            .expect("撤销"),
        2
    );
    assert!(
        !repos
            .grants
            .is_allowed(token_id, c1_id)
            .await
            .expect("查询")
    );

    // 按连接撤销
    repos.grants.set(token_id, c2_id, true).await.expect("授权");
    assert_eq!(
        repos
            .grants
            .revoke_all_for_connection(c2_id)
            .await
            .expect("撤销"),
        1
    );
    assert!(
        !repos
            .grants
            .is_allowed(token_id, c2_id)
            .await
            .expect("查询")
    );

    // 引用不存在的 Token 会被外键拒绝
    assert!(matches!(
        repos.grants.set(TokenId::new(), c1_id, true).await,
        Err(PersistenceError::NotFound { .. })
    ));

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn audit_filters_and_pagination() {
    let path = db_path();
    let repos = open(&path).await;

    let token_a = TokenId::new();
    let token_b = TokenId::new();
    let connection_id = ConnectionId::new();

    let first = repos
        .audit
        .append(sample_audit(
            AuditKind::CommandExecuted,
            token_a,
            connection_id,
            1_000,
        ))
        .await
        .expect("写入");
    repos
        .audit
        .append(sample_audit(
            AuditKind::SessionOpened,
            token_b,
            connection_id,
            2_000,
        ))
        .await
        .expect("写入");
    repos
        .audit
        .append(sample_audit(
            AuditKind::CommandExecuted,
            token_a,
            connection_id,
            3_000,
        ))
        .await
        .expect("写入");

    assert_eq!(repos.audit.count().await.expect("计数"), 3);
    let record = repos.audit.get(first).await.expect("读取").expect("应存在");
    assert_eq!(record.kind, AuditKind::CommandExecuted);
    assert_eq!(record.token_id, Some(token_a));
    assert_eq!(record.outcome, AuditOutcome::Allowed);
    assert_eq!(record.duration_ms, Some(5));

    let by_kind = AuditQuery {
        kind: Some(AuditKind::CommandExecuted),
        ..AuditQuery::default()
    };
    let page = repos.audit.query(&by_kind).await.expect("查询");
    assert_eq!(page.total, 2);
    assert_eq!(page.items[0].started_at, UnixMillis(3_000), "按时间倒序");

    let by_token = AuditQuery {
        token_id: Some(token_b),
        ..AuditQuery::default()
    };
    let page = repos.audit.query(&by_token).await.expect("查询");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].started_at, UnixMillis(2_000));

    // 时间闭区间
    let by_time = AuditQuery {
        from: Some(UnixMillis(1_000)),
        to: Some(UnixMillis(2_000)),
        ..AuditQuery::default()
    };
    let page = repos.audit.query(&by_time).await.expect("查询");
    assert_eq!(page.total, 2);

    // AND 组合：结果为空
    let denied = AuditQuery {
        outcome: Some(AuditOutcome::Denied),
        ..AuditQuery::default()
    };
    assert_eq!(repos.audit.query(&denied).await.expect("查询").total, 0);

    let paged = AuditQuery {
        offset: 1,
        limit: 1,
        ..AuditQuery::default()
    };
    let page = repos.audit.query(&paged).await.expect("查询");
    assert_eq!(page.total, 3, "total 不受分页影响");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].started_at, UnixMillis(2_000));

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn migration_splits_legacy_status_encoding() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    let path = db_path();
    // 先用 0001 建表并写入旧编码数据，再单独执行 0002，模拟已有数据库升级。
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("打开旧版数据库");
    sqlx::raw_sql(include_str!("../../../migrations/0001_init.sql"))
        .execute(&pool)
        .await
        .expect("执行 v1 建表");
    sqlx::query(
        "INSERT INTO audit_events (kind, started_at, status) \
         VALUES ('command_executed', 1000, 'allowed|succeeded')",
    )
    .execute(&pool)
    .await
    .expect("写入旧编码审计");
    sqlx::raw_sql(include_str!("../../../migrations/0002_audit_outcome.sql"))
        .execute(&pool)
        .await
        .expect("执行 v2 迁移");
    sqlx::raw_sql(include_str!(
        "../../../migrations/0005_audit_permission_context.sql"
    ))
    .execute(&pool)
    .await
    .expect("执行 v5 迁移");

    let (outcome, status): (String, String) =
        sqlx::query_as("SELECT outcome, status FROM audit_events WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("读取拆分后的列");
    assert_eq!(outcome, "allowed", "分隔符前应回填为 outcome");
    assert_eq!(status, "succeeded", "分隔符后应保留为 status");

    let repos = SqliteRepos::new(pool.clone());
    let record = repos
        .audit
        .get(AuditId(1))
        .await
        .expect("读取审计")
        .expect("审计应存在");
    assert_eq!(record.outcome, AuditOutcome::Allowed);
    assert_eq!(record.state, ExecutionState::Succeeded);

    pool.close().await;
    cleanup(&path);
}

#[tokio::test]
async fn audit_outcome_filter_matches_dedicated_column() {
    let path = db_path();
    let db = Database::connect(&path).await.expect("打开数据库应成功");
    let repos = SqliteRepos::new(db.pool().clone());

    let token_id = TokenId::new();
    let connection_id = ConnectionId::new();
    repos
        .audit
        .append(sample_audit(
            AuditKind::CommandExecuted,
            token_id,
            connection_id,
            1_000,
        ))
        .await
        .expect("写入 allowed");
    let denied = repos
        .audit
        .append(NewAuditEntry {
            outcome: AuditOutcome::Denied,
            kind: AuditKind::PermissionDenied,
            command: Some("open_session connection_id=abc".to_string()),
            state: ExecutionState::Failed,
            exit_code: None,
            duration_ms: None,
            error: Some("访问被拒绝".to_string()),
            ..sample_audit(AuditKind::PermissionDenied, token_id, connection_id, 2_000)
        })
        .await
        .expect("写入 denied");

    let page = repos
        .audit
        .query(&AuditQuery {
            outcome: Some(AuditOutcome::Denied),
            ..AuditQuery::default()
        })
        .await
        .expect("按 outcome 查询");
    assert_eq!(page.total, 1, "outcome 过滤必须命中且只命中 denied");
    assert_eq!(page.items[0].id, denied);
    assert_eq!(page.items[0].kind, AuditKind::PermissionDenied);
    assert_eq!(page.items[0].state, ExecutionState::Failed);

    // status 列只存执行状态，不再包含联合编码。
    let raw: String = sqlx::query_scalar("SELECT status FROM audit_events WHERE id = ?")
        .bind(denied.0)
        .fetch_one(db.pool())
        .await
        .expect("读取 status");
    assert_eq!(raw, "failed");

    drop(repos);
    drop(db);
    cleanup(&path);
}

#[tokio::test]
async fn close_all_by_token_only_affects_active_sessions() {
    let path = db_path();
    let repos = open(&path).await;

    let token_a = TokenId::new();
    let token_b = TokenId::new();
    let connection_id = ConnectionId::new();

    let s1 = SessionId::new();
    let s2 = SessionId::new();
    let s3 = SessionId::new();
    let s4 = SessionId::new();
    for (id, token, state, started_at) in [
        (s1, token_a, SessionState::Running, 1_000),
        (s2, token_a, SessionState::Connected, 2_000),
        (s3, token_a, SessionState::Closed, 3_000),
        (s4, token_b, SessionState::Running, 4_000),
    ] {
        repos
            .sessions
            .create(&sample_session(id, token, connection_id, state, started_at))
            .await
            .expect("创建");
    }

    // 重复主键 → Conflict
    assert!(matches!(
        repos
            .sessions
            .create(&sample_session(
                s1,
                token_a,
                connection_id,
                SessionState::Running,
                1_000,
            ))
            .await,
        Err(PersistenceError::Conflict(_))
    ));

    assert_eq!(repos.sessions.count_active().await.expect("计数"), 3);
    let closed = repos
        .sessions
        .close_all_by_token(token_a, "token 已撤销")
        .await
        .expect("关闭");
    assert_eq!(closed, 2, "已是 Closed 的会话不计数");

    for id in [s1, s2] {
        let info = repos.sessions.get(id).await.expect("读取").expect("应存在");
        assert_eq!(info.state, SessionState::Closed);
        assert!(info.ended_at.is_some());
        assert_eq!(info.close_reason.as_deref(), Some("token 已撤销"));
    }
    let other = repos.sessions.get(s4).await.expect("读取").expect("应存在");
    assert_eq!(other.state, SessionState::Running, "其他 Token 不受影响");

    let active = repos
        .sessions
        .list_active(PageRequest::default())
        .await
        .expect("列表");
    assert_eq!(active.total, 1);
    assert_eq!(active.items[0].id, s4);

    let history = repos
        .sessions
        .list_by_token(token_a, PageRequest::default())
        .await
        .expect("列表");
    assert_eq!(history.total, 3);
    assert_eq!(history.items[0].id, s3, "按启动时间倒序");

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn session_history_and_update_state() {
    let path = db_path();
    let repos = open(&path).await;

    let token_id = TokenId::new();
    let connection_id = ConnectionId::new();
    let s1 = SessionId::new();
    let s2 = SessionId::new();
    repos
        .sessions
        .create(&sample_session(
            s1,
            token_id,
            connection_id,
            SessionState::Closed,
            1_000,
        ))
        .await
        .expect("创建");
    repos
        .sessions
        .create(&sample_session(
            s2,
            token_id,
            connection_id,
            SessionState::Running,
            2_000,
        ))
        .await
        .expect("创建");

    let filter = SessionFilter {
        state: Some(SessionState::Closed),
        ..SessionFilter::default()
    };
    let page = repos.sessions.list_history(&filter).await.expect("查询");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, s1);

    // AND 组合与时间闭区间
    let filter = SessionFilter {
        token_id: Some(token_id),
        connection_id: Some(connection_id),
        from: Some(UnixMillis(1_500)),
        to: Some(UnixMillis(2_000)),
        ..SessionFilter::default()
    };
    let page = repos.sessions.list_history(&filter).await.expect("查询");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, s2);

    repos
        .sessions
        .update_state(
            s2,
            SessionState::Closed,
            Some(UnixMillis(3_000)),
            Some("正常退出".to_string()),
        )
        .await
        .expect("更新");
    let info = repos.sessions.get(s2).await.expect("读取").expect("应存在");
    assert_eq!(info.state, SessionState::Closed);
    assert_eq!(info.ended_at, Some(UnixMillis(3_000)));
    assert_eq!(info.close_reason.as_deref(), Some("正常退出"));
    assert_eq!(repos.sessions.count_active().await.expect("计数"), 0);

    assert!(matches!(
        repos
            .sessions
            .update_state(SessionId::new(), SessionState::Closed, None, None)
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    drop(repos);
    cleanup(&path);
}

#[tokio::test]
async fn secrets_settings_and_host_keys() {
    let path = db_path();
    let repos = open(&path).await;

    let connection = sample_connection("secret-holder", "ssh", 1_000);
    let connection_id = connection.id;
    repos
        .connections
        .create(connection)
        .await
        .expect("创建连接");

    let secret = SecretBlob {
        ciphertext: vec![1, 2, 3],
        nonce: vec![9],
        key_version: 2,
    };
    repos
        .secrets
        .put(connection_id, secret.clone())
        .await
        .expect("写入密文");
    assert_eq!(
        repos.secrets.get(connection_id).await.expect("读取"),
        Some(secret)
    );

    // 覆盖写入
    let replaced = SecretBlob {
        ciphertext: vec![4, 5],
        nonce: vec![7],
        key_version: 3,
    };
    repos
        .secrets
        .put(connection_id, replaced.clone())
        .await
        .expect("覆盖");
    assert_eq!(
        repos.secrets.get(connection_id).await.expect("读取"),
        Some(replaced)
    );

    // 连接不存在时被外键拒绝
    assert!(matches!(
        repos
            .secrets
            .put(
                ConnectionId::new(),
                SecretBlob {
                    ciphertext: vec![0],
                    nonce: vec![0],
                    key_version: 1,
                }
            )
            .await,
        Err(PersistenceError::NotFound { .. })
    ));

    assert!(repos.secrets.delete(connection_id).await.expect("删除"));
    assert!(
        !repos
            .secrets
            .delete(connection_id)
            .await
            .expect("再删返回 false")
    );
    assert_eq!(repos.secrets.get(connection_id).await.expect("读取"), None);

    repos.settings.set("mcp.port", "17321").await.expect("写入");
    repos.settings.set("ui.theme", "dark").await.expect("写入");
    repos.settings.set("mcp.port", "17322").await.expect("覆盖");
    assert_eq!(
        repos
            .settings
            .get("mcp.port")
            .await
            .expect("读取")
            .as_deref(),
        Some("17322")
    );
    let all = repos.settings.all().await.expect("全部");
    assert_eq!(
        all.keys().cloned().collect::<Vec<_>>(),
        vec!["mcp.port".to_string(), "ui.theme".to_string()]
    );

    assert!(
        repos
            .host_keys
            .get("10.0.0.5:22")
            .await
            .expect("读取")
            .is_none()
    );
    let record = sample_host_key();
    repos
        .host_keys
        .upsert(record.clone())
        .await
        .expect("写入 Host Key");
    assert_eq!(
        repos.host_keys.get("10.0.0.5:22").await.expect("读取"),
        Some(record)
    );

    let rotated = HostKeyRecord {
        fingerprint: "SHA256:def".to_string(),
        trusted_at: UnixMillis(2_000),
        trusted_by: Some("admin".to_string()),
        ..sample_host_key()
    };
    repos
        .host_keys
        .upsert(rotated.clone())
        .await
        .expect("覆盖 Host Key");
    assert_eq!(
        repos.host_keys.get("10.0.0.5:22").await.expect("读取"),
        Some(rotated)
    );

    drop(repos);
    cleanup(&path);
}
