# 密钥与凭据存储方案（T4 交付）

> 对应实现：`crates/security/**`。任何改动必须先更新本文档。

## 一、密钥放在哪里

| 材料 | 存放位置 | 条目名 |
|---|---|---|
| 连接凭据主密钥（32 字节） | Windows 凭据管理器（`keyring` v1，service = `com.universal-mcp-terminal`） | `secret-master-key` |
| Token 校验 pepper（32 字节） | 同上 | `token-pepper` |

- **禁止**把主密钥写入配置文件、SQLite 或日志。
- 首次启动时由 `OsKeyStore::get_or_create` 生成并写入；之后只读取。
- 密钥丢失等价于凭据不可恢复：不会静默重新生成覆盖已有密文，而是解密失败并提示。

## 二、连接凭据加密

- 算法：AES-256-GCM。
- 附加认证数据（AAD）固定为 `umt-v1`，防止密文被挪用到其他用途。
- 每条记录使用独立 96-bit 随机 nonce，与密文、`key_version` 一起持久化。
- `key_version` 当前为 1；轮换密钥时递增版本，并按版本号区分解密路径（旧版本保留只读密钥）。
- 解密失败一律视为 `CorruptedSecret`，不返回部分明文。

数据库落地字段（`connection_secrets`）：`ciphertext BLOB`、`nonce BLOB`、`key_version INTEGER`。

## 三、Token

- 明文结构：`umt_<8 位十六进制前缀>_<32 随机字节的 hex>`，熵 256 bit。
  随机体使用 hex 而非 base64url，因为后者的字母表包含 `_`，会破坏前缀解析。
- 服务端只保存：`prefix`（可识别、可索引）与 `hmac_hex`（HMAC-SHA256(pepper, 完整 Token)）。
- 校验使用 `subtle::ConstantTimeEq` 常量时间比较，避免时序侧信道。
- 明文只在创建接口返回一次，之后任何接口都不提供回读。
- 禁用、撤销、过期都在后端判断，前端展示不参与决策。

## 四、Host Key 策略

- 策略函数 `evaluate_host_key` 只做三态判断：`Trusted` / `Unknown` / `Mismatch`。
- 首次连接（`Unknown`）：桌面端弹出确认，用户确认后写入 `ssh_host_keys` 表。
- 无人值守的 MCP 调用遇到 `Unknown` **直接拒绝**，错误码 `access_denied`，要求人工在桌面端确认。
- 指纹变化（`Mismatch`）：阻断连接并提示可能的中人攻击，只能由用户在桌面端手动更新。

## 五、威胁模型与边界

| 场景 | 结果 |
|---|---|
| 攻击者只拿到 `umt.db` 文件 | 凭据为密文，无法解密 |
| 攻击者拿到同一 Windows 账户 | 可读取密钥 —— 属于同权限域内的必然结果，需靠系统账户隔离 |
| 日志/审计泄露 | 不记录密码、私钥、完整 Token；审计在写入前统一脱敏 |
| 前端被篡改 | 后端每次请求重新校验 Token 与授权，前端不作为安全边界 |

## 六、禁止事项

1. 禁止把密钥或 Token 明文写入任何日志、错误信息、数据库或前端存储。
2. 禁止在未提升 `key_version` 的情况下更换主密钥。
3. 禁止让 MCP 调用方读取或修改 `connection_secrets`。
