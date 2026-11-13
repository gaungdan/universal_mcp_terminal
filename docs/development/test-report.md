# Universal MCP Terminal 测试报告（T21）

> 运行环境：Windows，Rust workspace（`rust-toolchain.toml` 固定 `stable-x86_64-pc-windows-gnu`），
> Tauri 2 + React 19 + Vite 8 + Vitest 5。
> 运行日期：2026-11-13。以下结果全部来自本机真实命令输出，未手工修改数字。

## 一、门禁结果

| 命令 | 工作目录 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 仓库根 | **失败**：`crates/mcp-server/tests/http_e2e.rs:576` 有 1 处格式差异（T20 并行新增的 e2e 测试文件；T21 按约束不修改代码） |
| `cargo clippy --workspace --all-targets -j 1 -- -D warnings` | 仓库根 | **通过**：`Finished dev profile`，无 warning/error（1m34s） |
| `cargo test --workspace -j 1` | 仓库根 | **通过**：167 passed，0 failed，2 ignored |
| `npm run typecheck` | `apps/desktop` | **通过**：`tsc -b --noEmit`，退出码 0 |
| `npm run lint` | `apps/desktop` | **通过**：`eslint .`，退出码 0 |
| `npm run test` | `apps/desktop` | **通过**：10 个测试文件，66 个用例全部通过 |
| `npm run build` | `apps/desktop` | **通过**：60 个模块，`dist/assets/index-*.js` 347.77 kB（gzip 104.45 kB） |

说明：

- 第一次 `cargo test --workspace` 在 T20 的 `http_e2e.rs` 出现前执行，结果为 160 passed / 1 ignored；
  e2e 文件补齐后重新执行，最终结果为 167 passed / 2 ignored / 0 failed。
- `npm run test` 输出中存在 React `act(...)` 警告（`App.test.tsx` 渲染首页时异步状态更新），不影响用例通过。
- `cargo fmt` 的失败由并行工单新增文件引起；本工单未修改任何代码。

## 二、测试数量

### 2.1 Rust（`cargo test --workspace -j 1`）

| Crate / 目标 | 通过 | 忽略 | 说明 |
|---|---:|---:|---|
| `umt-core` | 18 | 0 | 领域模型、错误码、能力、session 状态机等 |
| `umt-protocol-api` | 9 | 0 | Schema 校验、错误映射、MockProvider |
| `umt-protocol-registry` | 2 | 0 | 注册、重复注册拒绝 |
| `umt-security` | 12 | 0 | AES-GCM、Token HMAC、Host Key 三态 |
| `umt-persistence` | 22 | 0 | 重启恢复、过滤分页、乐观锁、授权、审计、Host Key、0002 迁移 |
| `umt-audit` | 6 | 0 | 脱敏、CSV 导出 |
| `umt-access-control` | 6 | 0 | Token 生命周期、授权矩阵、撤销联动 |
| `umt-session-manager` | 14 | 0 | 状态机、跨 Token 隔离、空闲回收、执行策略（6 个策略用例） |
| `umt-provider-ssh`（单元） | 19 | 0 | Schema、能力、配置校验 |
| `umt-provider-ssh`（`tests/real_connection.rs`） | 0 | 1 | 真实 SSH 连接测试（默认忽略，需 `UMT_TEST_SSH_*`）；已用 WSL sshd + 密钥认证实测通过，见 `real-device-integration.md` |
| `umt-provider-telnet`（单元） | 4 | 0 | 能力、Schema、配置 |
| `umt-provider-telnet`（`tests/telnet_provider.rs`） | 12 | 0 | 假设备登录、exec、分页、超时、resize 拒绝 |
| `umt-provider-telnet`（`tests/real_telnet.rs`） | 0 | 1 | 真实 TCP 链路测试（需启动 `scripts/telnet_device_sim.py`）；已实测通过登录、exec 与自动分页 |
| `umt-provider-wsl` | 29 | 0 | 发行版枚举、UTF-16LE 解码、exec、超时、进程树清理、真实 WSL |
| `umt-mcp-server`（单元） | 15 | 0 | Bearer 解析、越权审计、能力、临时会话、密钥版本 |
| `umt-mcp-server`（`tests/http_e2e.rs`） | 8 | 0 | 真实 HTTP + Streamable HTTP；含未认证 `initialize` / `tools/list` 被 401 拒绝 |
| `umt-mcp-server`（`tests/official_client.rs`） | 2 | 0 | 官方 rmcp 客户端握手、9 工具、open/close session、错误 Token 拒绝 |
| `umt-app`（`tests/services.rs`） | 4 | 0 | 重启恢复、密文往返、未授权拒绝、Host Key 信任往返 |
| `umt-app` / `umt-desktop` / doc-tests | 0 | 0 | 无独立单测 |
| **合计** | **182** | **2** | 0 failed |

忽略的 2 个用例：

1. `umt-provider-ssh::real_ssh_test_connect_exec_and_interactive_shell`
   —— 需要 `UMT_TEST_SSH_HOST/PORT/USER` 与 `PASSWORD` 或 `PRIVATE_KEY`；已用 WSL sshd 实测通过。
2. `umt-provider-telnet::real_telnet_login_exec_and_paging`
   —— 需要先启动 `scripts/telnet_device_sim.py`；已实测通过登录、exec 与分页。

### 2.2 前端（Vitest）

10 个测试文件、66 个用例全部通过：

```text
src/test/errors.test.ts                      5
src/test/use-async.test.tsx                  2
src/test/components.test.tsx                 6
src/test/schema-form.test.tsx                9
src/App.test.tsx                             1
src/pages/settings/SettingsPage.test.tsx     6
src/pages/home/HomePage.test.tsx             7
src/pages/access-control/...test.tsx        10
src/pages/audit/audit.test.tsx              10
src/pages/connections/ConnectionsPage.test 10
```

## 三、需求 §15 验收标准逐条结论

### 3.1 数据持久化

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | 重启后连接仍然存在 | **已覆盖** | `crates/persistence/src/repo/sqlite/tests.rs::data_survives_restart`；`crates/app/tests/services.rs::services_survive_restart_and_secrets_roundtrip` |
| 2 | 重启后 Token 仍然存在 | **已覆盖** | 同上，Token 与 HMAC 一起校验 |
| 3 | 重启后连接授权关系仍然存在 | **已覆盖** | 同上，重启后 `is_allowed` 与授权列表均保留 |
| 4 | 重启后审计记录仍然存在 | **已覆盖** | `data_survives_restart` 写入并重新读取 `AuditKind::SessionOpened` |
| 5 | 数据升级不会无故丢失 | **部分覆盖** | 新增 0002 迁移与旧数据回填用例（`migration_splits_legacy_status_encoding`）；但只有 v1→v2 一条升级路径，没有回滚用例 |

### 3.2 协议

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | SSH 可真实认证并执行命令 | **未覆盖（自动化）** | 真实连接用例被 `#[ignore]`，需提供 `UMT_TEST_SSH_HOST/PORT/USER/PASSWORD`；本次未配置目标机 |
| 2 | Telnet 可完成登录交互并执行命令 | **部分覆盖** | 12 个假设备集成测试覆盖登录状态机、密码提示、分页、exec、超时；无真实网络设备自动化测试 |
| 3 | WSL 可调用指定发行版 | **已覆盖** | 本机存在 `Ubuntu`、`docker-desktop`；29 个 WSL 用例通过，含 `test_connection` 真实执行 `uname -a` |
| 4 | 三种协议都能创建和关闭 Session | **部分覆盖** | Telnet/WSL 的创建、读写、exec、close 有测试；SSH 的会话生命周期仅由被忽略的真实连接测试覆盖 |
| 5 | 连接失败能返回真实错误 | **已覆盖** | SSH/Telnet/WSL 均有连接失败、认证失败、超时的分类测试；e2e 验证 `access_denied` |
| 6 | 不支持的能力不会被伪装成成功 | **已覆盖** | `ensure_capability` 单测；Telnet `resize_is_unsupported_and_close_releases_socket` |

### 3.3 Token

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | 可以创建多个 Token | **已覆盖** | `AccessControl` 单测创建多个 Token；`tokens` 表允许任意数量 |
| 2 | 一个 Token 可以授权多个连接 | **已覆盖** | `grants_toggle_both_directions` 为一个 Token 授权/拒绝多个连接 |
| 3 | 一个连接可以授权多个 Token | **部分覆盖** | 表结构支持多对多，`list_tokens_for_connection` 有反向查询用例；但没有「同一连接显式授权给两个 Token」的用例 |
| 4 | 关闭授权后立即失效 | **已覆盖** | `set_grant` 后 `is_allowed` 立即为 false；`connection_authorization_follows_grant_and_enabled_state` |
| 5 | 撤销 Token 后立即失效 | **已覆盖** | `AccessControl` 单测；e2e `revoked_token_is_rejected_on_next_call` |
| 6 | 不同 Token 不能互相操作 Session | **已覆盖** | `SessionManager` 单测 + e2e `resize_terminal_with_forged_session_id_fails` / `another_token_cannot_touch_the_session` |
| 7 | 无法通过修改 connection_id 绕过权限 | **已覆盖** | 每次调用都走 `authorize_connection`；e2e `get_connection_info_rejects_unauthorized_connection_id` |

### 3.4 MCP

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | 标准 MCP 客户端能够连接 | **已覆盖** | e2e 真实 HTTP + `tests/official_client.rs` 使用 rmcp 官方客户端完成 initialize / list_tools / open_session / close_session；客户端默认 `2026-07-28`，服务端自动降级 `2025-11-25` 握手成功 |
| 2 | Bearer Token 能够正确认证 | **已覆盖** | e2e 全流程使用真实 Bearer；`parse_bearer` 单测覆盖大小写与畸形头 |
| 3 | 未认证请求被拒绝 | **已覆盖** | `/mcp` 前置 Bearer 中间件返回 401；`tools/call` 另有工具级二次校验 |
| 4 | 过期 Token 被拒绝 | **已覆盖** | `disabled_and_expired_tokens_are_rejected` |
| 5 | 已撤销 Token 被拒绝 | **已覆盖** | e2e `revoked_token_is_rejected_on_next_call` + 单测 |
| 6 | 未授权连接无法访问 | **已覆盖** | e2e 越权矩阵 + `AccessControl` 单测 |
| 7 | MCP 默认仅监听本机地址 | **已覆盖** | `McpServerConfig::default()` 为 `127.0.0.1:47321`；启动测试绑定 `127.0.0.1` |

### 3.5 审计

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | 每个 Session 有唯一 ID | **已覆盖** | `SessionId::new()`（UUID）；`sessions.id` 主键 |
| 2 | 每条命令有完整记录 | **部分覆盖** | 记录命令、状态、退出码、stdout/stderr 摘要；`working_dir` 在 `SessionManager::exec` 中固定写 `None`，输出截断 8192 字符 |
| 3 | 可以查询输入输出时间线 | **部分覆盖** | `InputSent` 与 `CommandExecuted` 有记录；`read_output` 未写审计，`AuditKind::OutputRead` 目前未使用 |
| 4 | 可以查看失败原因 | **已覆盖** | `error` 字段记录失败原因；脱敏后展示 |
| 5 | 可以区分超时和执行失败 | **已覆盖** | `ExecutionState::TimedOut` / `Failed` / `Unknown`；Telnet/WSL 超时测试 |
| 6 | 敏感信息得到脱敏 | **已覆盖** | `Redactor` 单测覆盖 password、Bearer、`umt_` Token、PEM 私钥、SSH 公钥 |
| 7 | 审计记录持久化 | **已覆盖** | SQLite `data_survives_restart`；审计仓库只追加 |

补充：权限拒绝事件已落库（`kind = permission_denied`、`outcome = denied`），
由 `unauthorized_connection_writes_permission_denied_audit` 覆盖；`outcome` 已成为独立列，
按结果过滤直接命中该列。

### 3.6 安全

| # | 验收项 | 结论 | 依据/原因 |
|---|---|---|---|
| 1 | 连接密码不能明文保存 | **已覆盖** | `connection_secrets` 只存 AES-256-GCM 密文 + nonce + key_version；`secret_cipher_roundtrip_and_nonce_validation` |
| 2 | Token 不能明文保存 | **已覆盖** | 只存 `prefix` + HMAC-SHA256；`generated_token_verifies` / `token_has_256_bit_entropy_source` |
| 3 | 私钥不能进入普通日志 | **已覆盖（代码审查）** | 无日志打印私钥；`Redactor` 覆盖 PEM 块；SSH Provider 只把私钥用于 `decode_secret_key` |
| 4 | 未授权操作被后端拒绝 | **已覆盖** | 工具层 `authorize_connection` + 会话归属校验；e2e 越权矩阵 |
| 5 | Token 撤销后活动 Session 被终止 | **已覆盖** | `SessionManager::revoke_token_sessions` 单测；关闭原因 `token_revoked` |
| 6 | 高风险操作按策略要求用户确认 | **部分覆盖** | `CommandPolicy` 已按 ReadOnly/Restricted/FullAccess 拦截命令，高风险命令写 `permission_denied` 审计并拒绝；MCP 通道暂无交互式确认，`NeedsConfirmation` 统一按拒绝处理 |
| 7 | Host Key 确认与指纹固定 | **已覆盖** | 连接测试返回观察到的指纹；`trust_host_key` / `list_host_keys` / `delete_host_key` 三个 IPC 命令 + 连接详情页确认按钮 + 设置页撤销，`host_key_trust_list_and_revoke_roundtrip` 覆盖往返 |

## 四、已知技术债与缺口

1. **`audit_events.outcome` 编码（已修复）**
   0002 迁移新增独立 `outcome` 列并把旧 `status`（`allowed|succeeded`）拆分为
   `outcome` + `status`；SQLite 仓库的写入、过滤、读取均直接使用该列，`status` 只保存执行状态。
   回归用例：`migration_splits_legacy_status_encoding`、`audit_outcome_filter_matches_dedicated_column`。

2. **MCP HTTP 层认证（已修复）**
   已在 `/mcp` 路由前置 Bearer 中间件（`crates/mcp-server/src/service.rs`），
   未认证请求在进入 rmcp 前直接返回 401；`UmtMcpServer::call_tool` 内仍保留工具级二次校验。

3. **Telnet 选项集未完整实现**
   当前实现覆盖登录提示、密码提示、命令提示符、`--More--` 分页与 IAC 过滤；
   未实现完整 Telnet 选项协商，且未声明 `TerminalResize`。真实设备的提示符差异仍需人工验证。

4. **WSL 仅支持 Windows**
   `list_distributions` 在非 Windows 返回 `ProviderError::Internal("仅支持 Windows")`；
   `WslProvider` 依赖 `wsl.exe`，无法在 Linux/macOS 构建上提供真实能力。

5. **Host Key 桌面端确认流程（已修复）**
   SSH 严格校验与「未知指纹拒绝」保持不变；新增 `list_host_keys` / `trust_host_key` /
   `delete_host_key` 三个 Tauri 命令，连接详情页在测试结果中展示服务端指纹并可一键信任，
   设置页可查看与撤销已信任指纹。`endpoint` 由后端按 `host:port` 推导，前端不拼接。

6. **安全执行策略（部分落地）**
   `ReadOnly` / `Restricted` / `FullAccess` 与高风险识别已接入 `SessionManager::exec`，
   拒绝时写 `PermissionDenied` 审计并立即生效；设置修改后热更新，不再需要重启。
   限制：MCP 通道没有交互式确认能力，`NeedsConfirmation` 统一按拒绝处理；
   字符串策略不能替代远端受限账号与系统权限控制。

7. **审计完整性（部分修复）**
   `AuditKind::PermissionDenied` 已接入：MCP 越权与能力不支持失败会落
   `permission_denied` / `denied` 审计（含认证失败时的 `token_id = None`）。
   仍存在：`read_output` 与 `resize_terminal` 不写审计；`exec` 审计的 `working_dir` 固定为 `None`。

8. **SSH 真实连接未进入默认门禁**
   测试 `#[ignore]`，CI/本地默认运行不会验证真实 SSH；需要提供测试服务器环境变量。

9. **数据升级无自动化验证（已补充 v1→v2）**
   0002 迁移已有「旧库建表 → 写入旧编码 → 执行迁移 → 校验拆列」的自动化用例；
   仍无回滚（down migration）验证。

10. **MCP 兼容性测试范围**
    e2e 使用 `reqwest` 手工构造 JSON-RPC/SSE，未使用官方 MCP 客户端或 `mcp-inspector`。

## 五、安全自查（日志/错误信息泄露扫描）

### 5.1 扫描范围与规则

范围：`crates/**`、`apps/desktop/src/**`、`apps/desktop/src-tauri/**`、`docs/**`，
排除 `node_modules/`、`target/`、`backup/`、`dist/`。

| 检查项 | 模式 | 结果 |
|---|---|---|
| 日志宏中包含敏感关键词 | `tracing::(trace\|debug\|info\|warn\|error)!` / `println!` / `eprintln!` 与 `password\|private_key\|passphrase\|secret\|token\|bearer\|authorization` 同行 | 仅 1 处：`crates/providers/ssh/tests/real_connection.rs:34` 打印环境变量**名称** `UMT_TEST_SSH_PASSWORD`，不打印值 |
| 硬编码完整 Token | `umt_[0-9a-fA-F]{8}_[0-9a-fA-F]{64}` | 无匹配 |
| 配置/文档中的真实 Bearer | `Authorization: Bearer <非占位值>` | 仅 `docs/mcp/接入指南.md` 与需求文档中的 `YOUR_ACCESS_TOKEN` 占位符 |
| 前端控制台/本地存储泄露 | `console.*` / `localStorage` / `sessionStorage` 与 `token\|password\|secret\|bearer` 同行 | 无匹配 |
| 生产代码中的裸 `println!` / `dbg!` | `println!` / `dbg!` | 仅 WSL 测试的“跳过”提示、`crates/app/build.rs` 的 `cargo:` 指令、SSH 测试的环境变量名提示 |

### 5.2 关键防护点（代码审查）

| 文件 | 防护 |
|---|---|
| `crates/audit/src/redact.rs` | 统一脱敏 password/passwd/pwd/passphrase/secret/token/api_key、`Bearer ...`、`umt_...`、PEM 私钥块、SSH 公钥行 |
| `crates/audit/src/service.rs` | 命令、工作目录、stdout/stderr、error 全部先脱敏再落库；输出截断 8192 字符 |
| `crates/mcp-server/src/handler.rs` | 日志只记录 `code` 与脱敏后的 `message`；不记录 Authorization 原文 |
| `crates/protocol-api/src/session.rs` | `SecretValues` 的 `Debug` 固定输出 `<redacted:n>` |
| `crates/providers/telnet/src/config.rs` | `TelnetConfig` 的 `Debug` 把 password 输出为 `<redacted>` |
| `crates/security/src/token.rs` | Token 明文用 `Zeroizing<String>`；只存前缀与 HMAC；常量时间比较 |
| `crates/security/src/cipher.rs` | AES-256-GCM，AAD 固定 `umt-v1`，nonce 12 字节 |
| `crates/app/src/services/connections.rs` | 敏感字段从配置中剥离后单独加密，连接详情只回显字段名 |
| `apps/desktop/src/**` | 未发现把 Token/密码写入 `console` 或浏览器存储 |

### 5.3 结论

**未发现生产代码把密码、私钥或完整 Token 写入日志、错误信息、前端控制台或浏览器存储。**
审计写入前统一脱敏，MCP 错误只保留错误码与脱敏文本；测试中的敏感字符串均为 fixture。

残余风险：

1. 脱敏是**基于模式**的，未命中规则的自由文本仍可能进入审计（例如命令输出中的自定义密钥格式）。
2. 错误信息若被 Provider 拼进凭据内容，仍可能泄露；当前 Provider 实现未这样做，但没有自动化“日志快照”测试兜底。
3. 完整 Token 会短暂存在于创建响应与前端内存中，这是“仅展示一次”设计的必然结果；不应写入持久化存储或日志。

## 六、审计缺口收口（2026-11-13 补充）

### 6.1 迁移 0002：`outcome` 独立成列

- 新增 `crates/persistence/migrations/0002_audit_outcome.sql`：
  `ALTER TABLE audit_events ADD COLUMN outcome TEXT NOT NULL DEFAULT 'allowed'`；
  用 `instr` / `substr` 把旧 `status`（形如 `allowed|succeeded`）拆成 `outcome` 与新的 `status`，
  只处理分隔符位于中间的行；新增索引 `idx_audit_outcome`（`IF NOT EXISTS`）。
- SQLite 仓库 `crates/persistence/src/repo/sqlite/audit.rs` 的写入、outcome 过滤与读取均使用独立列，
  `status` 只保存 `ExecutionState`；读取保留一次兼容兜底，历史值 `allowed|succeeded` 取前缀。

### 6.2 权限拒绝审计落点

- `UmtMcpServer::execute_tool` 包装统一分发：返回 `UmtError::AccessDenied` 或
  `UmtError::UnsupportedCapability` 时写入 `AuditKind::PermissionDenied` +
  `AuditOutcome::Denied` + `ExecutionState::Failed`。
- `command` 只包含工具名与 `connection_id` / `session_id`，不含命令正文、输入或凭据；
  `error` 由 `AuditService` 二次脱敏；审计写入失败只 `tracing::warn!`，不覆盖原始错误。
- `call_tool` 在认证阶段失败时也会写审计，`token_id = None`；HTTP 层 Bearer 中间件会在进入
  rmcp 前直接返回 401，因此该分支仅在直接调用 handler 时触发。

### 6.3 本次门禁与用例

| 命令 | 结果 |
|---|---|
| `cargo fmt --all` | **通过**，无格式差异 |
| `cargo test -p umt-persistence -j 1` | **通过**：22 passed / 0 failed（新增 2 个用例） |
| `cargo test -p umt-mcp-server -j 1` | **通过**：单元 15 passed、`http_e2e` 8 passed，0 failed |
| `cargo clippy -p umt-persistence -p umt-mcp-server --all-targets -j 1 -- -D warnings` | **被外部改动阻塞**：`crates/persistence/src/repo/memory.rs:707` 的 `sort_by` 触发 `clippy::unnecessary_sort_by`；该文件是并行工单的未提交改动，不在本工单写入范围。追加 `-A clippy::unnecessary_sort_by` 后，本工单涉及代码零 warning |

新增用例：

- `migration_splits_legacy_status_encoding`：用 0001 建旧库、写入 `allowed|succeeded`，执行 0002 后
  校验 `outcome = allowed`、`status = succeeded`。
- `audit_outcome_filter_matches_dedicated_column`：写入 allowed/denied 两条记录，
  按 `outcome = Denied` 只命中一条，并确认 `status` 不再含 `|`。
- `unauthorized_connection_writes_permission_denied_audit`：未授权连接调用后，
  `AuditRepo::query` 命中一条 `permission_denied` / `denied` 记录，含 `token_id`、连接 ID 摘要与错误原因。

非法 Token 由既有 e2e `invalid_bearer_token_is_rejected` 覆盖：请求在 `/mcp` Bearer 中间件被 401 拒绝，
不会 panic；该路径未进入 handler，因此**不落审计**。
