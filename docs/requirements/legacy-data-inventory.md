# 旧版数据盘点与迁移评估（T0 交付）

> 本文件只做事实记录与方案建议，**不包含任何凭据内容**，也未对旧数据做任何写操作。
> 备份目录：`backup/legacy-data-20261112/`（已在 `.gitignore` 中排除，禁止入库）

## 一、备份清单（SHA256 / 字节数）

| 文件 | 字节 | SHA256（前 16 位） |
|---|---|---|
| `umt.db` | 4 096 | `89a2318c76452be8` |
| `umt.db-wal` | 3 040 592 | `3fe65527bd3e890a` |
| `umt.db-shm` | 32 768 | `bb3e5903def1bb82` |
| `umt-pre-migration-v1-1794363908.db` | 110 592 | `abe6e994852db587` |
| `umt-pre-migration-v1-1794363908.db.sha256` | 65 | `507989406c028931` |
| `recordings/*.umtrec` | 8 ~ 37 870 | 共 6 个文件 |
| `audit.key` | 65 | `4dc7965d9b1eb5db`（凭据材料，禁止输出内容） |
| `ipc.token` | 65 | `6427e13860ef5898`（凭据材料，禁止输出内容） |

原始位置：`%LOCALAPPDATA%\UniversalMCPTerminal\`（另有一个 WebView 缓存目录 `com.universal-mcp-terminal.phase0`，无业务数据）。

## 二、旧库结构与实际数据量

旧库 `schema_migrations` 已执行到 **version 7**，共 15 张表：

| 表 | 行数 | 说明 |
|---|---|---|
| `devices` | 3 | 设备（含软删除字段） |
| `connection_profiles` | 3 | 连接配置，`driver_id` 为 `local.wsl` / `remote.ssh` |
| `credential_refs` | 2 | 仅存 `kind=password` 与 `target`，**不含密文** |
| `device_tags` | 0 | 标签 |
| `ssh_host_keys` | 0 | 已固定的 Host Key 指纹 |
| `terminal_sessions` | 4 | 均为 `closed`，4 条带 `.umtrec` 录制 |
| `audit_events` | 99 | 含 `prev_hash` / `event_hash` / `mac` 哈希链 |
| `connection_events` | 14 | `local.wsl` 13 次 `failed / driver_config_invalid`，1 次成功 |
| `executions` / `approvals` / `file_*` | 0 | 未使用 |
| `mcp_clients` | 0 | Token 表（旧版未创建客户端） |
| `schema_migrations` | 7 | — |

实际数据示例（不含敏感值）：设备 `Ubuntu`（wsl）、`Ubuntu`（ssh 172.27.39.33，用户名 guang）、`2342`（已删除）；审计动作含 `terminal.replayed` 43 次、`core.started` 9 次、`mcp_http.started/stopped` 各 3 次。

## 三、关键发现（直接解释"数据恢复失败"）

1. **主库文件只有 4 KB，3 MB 数据全部滞留在 WAL 中且未 checkpoint。** 只要复制/恢复时漏掉 `umt.db-wal`（或启动时用只读副本打开），界面就会表现为"数据全没了"。这是最可能的直接原因。
2. **密码不在数据库里。** `credential_refs.target` 形如 `umt/devices/<id>/ssh-password`，真实密码存放在 Windows 凭据管理器。即使数据库恢复成功，凭据条目丢失同样会导致"连接存在但认证失败"。
3. **旧版已经有迁移痕迹**：存在 `umt-pre-migration-v1-*.db`，而主库审计表只剩 99 条（迁移前库仅 1 条），说明历史上至少做过一次版本迁移并发生过数据形态变化。
4. **审计使用哈希链 + MAC**（`audit.key`），旧记录无法脱离该密钥校验；新审计模型必须明确"延续链"还是"另起链"。
5. **旧 WSL 配置校验有缺陷**：13 次 `driver_config_invalid`，与用户反馈的 WSL 连接失败一致，新 Provider 需要重新设计 `config_json` 校验。
6. 旧版功能范围比需求文档更宽（设备/标签/文件根目录/审批/录制回放）。**新首期不做文件传输**，旧 `file_*` 表数据可忽略。

## 四、旧 → 新 数据映射

| 旧表 | 新表 | 处理方式 |
|---|---|---|
| `devices` + `connection_profiles` | `connections` | 合并：`devices.name/group_name` 作名称与标签，`profiles.endpoint/username/config_json` 转协议配置；`local.wsl` → `wsl`，`remote.ssh` → `ssh` |
| `credential_refs` | `connection_secrets` | 需从 Windows 凭据管理器读出明文 → 用新 `SecretCipher` 重新加密；读不到则标记"需重新录入密码" |
| `ssh_host_keys` | `ssh_host_keys`（新增的表，见下） | 直接迁移，保留信任时间 |
| `terminal_sessions` | `sessions`（历史只读） | 仅作历史展示；活动会话不迁移 |
| `audit_events` | `audit_events` | 导入为 `source='legacy'` 的历史记录，保留原动作/结果/时间；不伪造新哈希链 |
| `mcp_clients` | `tokens` | **不可迁移**：只存了 `token_hash`，无法反推明文，用户必须在新 UI 重新创建 Token |
| `recordings/*.umtrec` | 不做首期迁移 | 与首期范围（无文件传输、无回放要求）无关，保留在备份中 |

> 新 schema 需补一张表（原方案遗漏，见执行方案 §2.3 的修订）：
>
> ```sql
> ssh_host_keys(endpoint TEXT PRIMARY KEY, host TEXT NOT NULL, port INTEGER NOT NULL,
>   algorithm TEXT, fingerprint TEXT NOT NULL, trusted_at INTEGER NOT NULL, trusted_by TEXT);
> ```

## 五、迁移策略建议（需用户确认后执行）

**推荐：新库 + 导入器，旧库永不写。**

1. 新库固定为 `%LOCALAPPDATA%\UniversalMCPTerminal\umt-v2.db`，首次启动自动建库、自动迁移到最新版本。
2. 旧 `umt.db`（含 wal/shm）只作为**只读数据源**，由一次性导入器读取；导入前再次校验 SHA256 与本次备份一致。
3. 导入结果写入 `import_log`（新增表），记录源文件哈希、导入条数、失败原因，确保可重复执行且幂等。
4. 导入完成前**不删除、不重命名、不 checkpoint 旧库**；用户确认后才允许归档旧文件。
5. Token 无法迁移，导入后引导用户重新创建并授权。

## 六、待用户确认的决策

| 编号 | 决策 | 建议 |
|---|---|---|
| D1 | 是否导入旧连接与历史审计 | 建议导入（3 个连接 + 99 条审计），密码能读则迁移，读不到标记待补录 |
| D2 | 旧审计哈希链如何处置 | 建议 `source='legacy'` 原样展示，不参与新链校验，UI 明确标注"历史记录" |
| D3 | 旧库文件处置 | 建议保留原样，仅在导入成功且用户确认后归档到 `backup/` |
| D4 | 是否迁移旧录制文件 | 首期不建议（需求首期不含回放） |
