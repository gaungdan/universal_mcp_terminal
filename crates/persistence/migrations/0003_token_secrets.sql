-- Token 明文的加密副本（仅用于在本地生成 MCP 配置，不提供回读接口）。
-- 与 tokens 一对一：删除 Token 时同步删除。
CREATE TABLE token_secrets (
    token_id    TEXT    PRIMARY KEY REFERENCES tokens(id) ON DELETE CASCADE,
    ciphertext  BLOB    NOT NULL,
    nonce       BLOB    NOT NULL,
    key_version INTEGER NOT NULL
);
