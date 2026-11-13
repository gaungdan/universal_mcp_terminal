-- Schema v1（对应 docs/development/multi-agent-execution-plan.md §2.3，已去掉 import_log）
-- WAL / foreign_keys / busy_timeout 由 Database::connect 的连接参数设置，
-- 迁移在事务内执行，故此处不再重复 PRAGMA。

CREATE TABLE connections (
    id           TEXT    PRIMARY KEY,
    name         TEXT    NOT NULL UNIQUE,
    protocol_id  TEXT    NOT NULL,
    description  TEXT,
    tags_json    TEXT    NOT NULL DEFAULT '[]',
    notes        TEXT,
    enabled      INTEGER NOT NULL DEFAULT 1,
    -- 非敏感协议字段；敏感字段统一放 connection_secrets
    config_json  TEXT    NOT NULL DEFAULT '{}',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    revision     INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_connections_protocol ON connections(protocol_id);
CREATE INDEX idx_connections_enabled ON connections(enabled);

CREATE TABLE connection_secrets (
    connection_id TEXT PRIMARY KEY REFERENCES connections(id) ON DELETE CASCADE,
    ciphertext    BLOB    NOT NULL,
    nonce         BLOB    NOT NULL,
    key_version   INTEGER NOT NULL
);

CREATE TABLE tokens (
    id           TEXT    PRIMARY KEY,
    name         TEXT    NOT NULL,
    description  TEXT,
    prefix       TEXT    NOT NULL UNIQUE,
    hmac         TEXT    NOT NULL,
    expires_at   INTEGER,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    status       TEXT    NOT NULL CHECK (status IN ('active', 'disabled', 'revoked'))
);

CREATE INDEX idx_tokens_status ON tokens(status);

CREATE TABLE token_connection_grants (
    token_id      TEXT    NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
    connection_id TEXT    NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    allowed       INTEGER NOT NULL DEFAULT 1,
    granted_at    INTEGER NOT NULL,
    PRIMARY KEY (token_id, connection_id)
);

-- 双向查询：主键已覆盖「按 token 查」，这里补「按 connection 反查」
CREATE INDEX idx_grants_connection ON token_connection_grants(connection_id, token_id);

-- sessions 故意不建外键：历史记录需要在连接/Token 删除后仍然保留
CREATE TABLE sessions (
    id            TEXT    PRIMARY KEY,
    token_id      TEXT    NOT NULL,
    connection_id TEXT    NOT NULL,
    protocol_id   TEXT    NOT NULL,
    client_info   TEXT,
    state         TEXT    NOT NULL,
    started_at    INTEGER NOT NULL,
    ended_at      INTEGER,
    close_reason  TEXT,
    rows          INTEGER,
    cols          INTEGER
);

CREATE INDEX idx_sessions_token ON sessions(token_id, started_at DESC);
CREATE INDEX idx_sessions_state ON sessions(state, started_at DESC);

CREATE TABLE audit_events (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id     TEXT,
    token_id       TEXT,
    connection_id  TEXT,
    kind           TEXT    NOT NULL,
    command        TEXT,
    working_dir    TEXT,
    started_at     INTEGER NOT NULL,
    duration_ms    INTEGER,
    status         TEXT    NOT NULL,
    exit_code      INTEGER,
    stdout_excerpt TEXT,
    stderr_excerpt TEXT,
    error          TEXT,
    redacted       INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_audit_started_at ON audit_events(started_at DESC);
CREATE INDEX idx_audit_token ON audit_events(token_id, started_at DESC);
CREATE INDEX idx_audit_connection ON audit_events(connection_id, started_at DESC);
CREATE INDEX idx_audit_session ON audit_events(session_id, started_at DESC);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE ssh_host_keys (
    endpoint    TEXT    PRIMARY KEY,
    host        TEXT    NOT NULL,
    port        INTEGER NOT NULL,
    algorithm   TEXT,
    fingerprint TEXT    NOT NULL,
    trusted_at  INTEGER NOT NULL,
    trusted_by  TEXT
);
