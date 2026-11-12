# Universal MCP Terminal — 多 Agent 并行开发执行方案

> 需求来源：[universal-mcp-terminal.md](../requirements/universal-mcp-terminal.md)
> 本文档解决三个问题：**现状怎么判断**、**架构怎么切**、**任务怎么拆给多个 agent 并行执行而不互相踩踏**。

---

## 一、现状分析（必须先读）

### 1.1 工作区实际状态

对 `d:\develop\GitHub\universal_mcp_terminal` 全量扫描结果：

| 项目 | 实际情况 | 影响 |
|---|---|---|
| 源码 | **不存在**（无 `Cargo.toml`、无 `crates/`、无 `frontend/`） | 无法做"先阅读现有项目"的分析，也无法定位旧版数据恢复 bug 的根因 |
| 构建产物 | 仅 `target/debug/{umt-core.exe, umt-desktop.exe, WebView2Loader.dll}` | 只能作为"旧版本存在过"的证据；二进制行为不可维护 |
| `apps/desktop/dist` | 空目录 | 前端产物已丢失 |
| Git 仓库 | **未初始化**（`fatal: not a git repository`） | 多 agent 并行没有版本回退能力，风险高 |

结论：**本次按"全新工程重建"推进**，但需求文档第三章要求的"数据恢复问题排查"必须转化为一个具体动作——**找到旧版应用数据目录并原样备份**，而不是凭空猜测旧序列化逻辑。

### 1.2 旧数据排查清单（T0 执行，只备份不修改）

1. `%APPDATA%\<app-identifier>`、`%LOCALAPPDATA%\<app-identifier>`、`%APPDATA%\universal-mcp-terminal`
2. 安装目录旁的 `data/`、`*.db`、`*.sqlite`、`*.json` 文件
3. `umt-core.exe` 的字符串特征（`ConnectionService`、`database`、`.db` 等）反推存储位置
4. 备份到 `backup/legacy-data-<yyyyMMdd>/`，记录文件清单与哈希，**不做任何写入或迁移**

只要旧数据存在，就写一份《旧数据格式说明 + 迁移脚本设计》，交由用户确认后才执行迁移；旧数据不存在则本阶段直接标记为"无历史包袱"，进入全新 schema。

### 1.3 需求内部的一个需要澄清点

需求 §5.2 要求"Host Key 首次连接提示用户确认并保存指纹"，但 §7 的 MCP 工具是无人值守调用。建议默认策略：**MCP 会话遇到未知 Host Key 一律拒绝并返回需人工确认的错误**，指纹只能由桌面端 UI 确认写入。此决策在 T0 固化到文档，避免 SSH provider 与 MCP 层各自实现一套。

---

## 二、架构切分（并行开发的前提）

### 2.1 Crate 边界与依赖方向

依赖只能**向下**，禁止反向依赖，禁止横向依赖：

```text
umt-app (Tauri commands / 初始化 / 生命周期)
  ├── umt-mcp-server ──┬── umt-access-control ──┐
  │                    ├── umt-session-manager ─┤
  │                    └── umt-protocol-registry │
  ├── umt-audit ────────────────────────────────┤──> umt-persistence
  ├── umt-access-control                        │
  ├── umt-session-manager                       │
  └── umt-protocol-registry                     │
        └── providers: ssh / telnet / wsl       │
              └── umt-protocol-api ─────────────┘
                    └── umt-core
umt-security (被 access-control / persistence / providers-ssh 使用)
```

硬性规则（评审时逐条检查）：

1. `umt-core`、`umt-protocol-api` **不依赖** tokio/网络/数据库，只依赖 serde。
2. `umt-session-manager`、`umt-mcp-server`、`umt-audit`、`umt-access-control` **不得** `use umt_provider_ssh::*`。
3. Provider crate **不得**依赖 persistence / audit / access-control。
4. 前端不得出现 `if (protocol === 'SSH')` 表单分支，一律走 Provider 下发的 JSON Schema。
5. 协议能力由 `Capability` 枚举表达，MCP 工具在调用前用能力表检查，不支持的能力返回 `UnsupportedCapability` 错误。

### 2.2 必须"先冻结"的 5 份契约

并行开发失败的根因几乎都是契约漂移。**Wave 0 结束时以下文件必须存在且被标记 `CONTRACT-FROZEN`，之后任何改动都走主控统一提交**：

| 契约 | 文件 | 内容 |
|---|---|---|
| C1 领域模型 | `crates/core/src/{ids,connection,capability,error,events}.rs` | 强类型 ID、Connection 汇总结构、`UmtError` 枚举、应用事件 |
| C2 协议接口 | `crates/protocol-api/src/{lib,provider,session,schema,test_result,error}.rs` | `ProtocolProvider`、`TerminalSession`、`ConfigSchema`、`ConnectionTestResult` |
| C3 存储契约 | `crates/persistence/src/{schema.sql,repos/*.rs}` | 建表 SQL + 仓库 trait（`ConnectionRepo`、`TokenRepo`、`GrantRepo`、`SessionRepo`、`AuditRepo`、`SettingsRepo`） |
| C4 安全契约 | `crates/security/src/{cipher,token,hostkey}.rs` | `SecretCipher`、`TokenHasher`、`HostKeyVerifier` trait + `TokenContext` |
| C5 IPC 契约 | `crates/app/src/ipc/dto.rs` + `apps/desktop/src/api/types.ts` + `apps/desktop/src/api/client.ts` | 命令名、请求/响应 DTO、错误码；两端字段逐一对齐 |

契约样例（冻结版本，agent 不得擅自扩展字段）：

```rust
// C2: 协议接口（异步、可 resize、可关闭，Session 由 Box 持有）
#[async_trait]
pub trait ProtocolProvider: Send + Sync {
    fn protocol_id(&self) -> &'static str;
    fn display_name(&self) -> &str;
    fn capabilities(&self) -> &CapabilitySet;
    fn config_schema(&self) -> ConfigSchema;
    async fn validate_config(&self, cfg: &ConnectionConfig) -> Result<(), ProviderError>;
    async fn test_connection(&self, cfg: &ConnectionConfig) -> Result<ConnectionTestResult, ProviderError>;
    async fn connect(&self, cfg: &ConnectionConfig, opts: ConnectOptions) -> Result<Box<dyn TerminalSession>, ProviderError>;
}

#[async_trait]
pub trait TerminalSession: Send {
    fn session_id(&self) -> SessionId;
    fn capabilities(&self) -> &CapabilitySet;
    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError>;
    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError>;
    /// 增量读取：返回游标之后的数据，None 表示流已结束
    async fn read(&mut self, cursor: u64) -> Result<Option<OutputChunk>, SessionError>;
    async fn close(self: Box<Self>) -> Result<(), SessionError>;
}
```

### 2.3 SQLite Schema v1（C3 冻结版）

```sql
PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;

connections(id TEXT PK, name TEXT NOT NULL UNIQUE, protocol_id TEXT NOT NULL,
  description TEXT, tags_json TEXT NOT NULL DEFAULT '[]', notes TEXT,
  enabled INTEGER NOT NULL DEFAULT 1, config_json TEXT NOT NULL,   -- 非敏感字段
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, revision INTEGER NOT NULL DEFAULT 1);

connection_secrets(connection_id TEXT PK REFERENCES connections(id) ON DELETE CASCADE,
  ciphertext BLOB NOT NULL, nonce BLOB NOT NULL, key_version INTEGER NOT NULL);

tokens(id TEXT PK, name TEXT NOT NULL, description TEXT, prefix TEXT NOT NULL UNIQUE,
  hmac TEXT NOT NULL, expires_at INTEGER, created_at INTEGER NOT NULL,
  last_used_at INTEGER, status TEXT NOT NULL CHECK(status IN ('active','disabled','revoked')));

token_connection_grants(token_id TEXT REFERENCES tokens(id) ON DELETE CASCADE,
  connection_id TEXT REFERENCES connections(id) ON DELETE CASCADE,
  allowed INTEGER NOT NULL DEFAULT 1, granted_at INTEGER NOT NULL,
  PRIMARY KEY(token_id, connection_id));

sessions(id TEXT PK, token_id TEXT NOT NULL, connection_id TEXT NOT NULL,
  protocol_id TEXT NOT NULL, client_info TEXT, state TEXT NOT NULL,
  started_at INTEGER NOT NULL, ended_at INTEGER, close_reason TEXT, rows INTEGER, cols INTEGER);

audit_events(id INTEGER PK AUTOINCREMENT, session_id TEXT, token_id TEXT, connection_id TEXT,
  kind TEXT NOT NULL, command TEXT, working_dir TEXT, started_at INTEGER NOT NULL,
  duration_ms INTEGER, status TEXT NOT NULL, exit_code INTEGER,
  stdout_excerpt TEXT, stderr_excerpt TEXT, error TEXT, redacted INTEGER NOT NULL DEFAULT 0);

settings(key TEXT PK, value TEXT NOT NULL);

-- 以下两张表为旧版数据盘点后补充（见 docs/requirements/legacy-data-inventory.md）
ssh_host_keys(endpoint TEXT PK, host TEXT NOT NULL, port INTEGER NOT NULL,
  algorithm TEXT, fingerprint TEXT NOT NULL, trusted_at INTEGER NOT NULL, trusted_by TEXT);

import_log(id INTEGER PK AUTOINCREMENT, source_path TEXT NOT NULL, source_sha256 TEXT NOT NULL,
  imported_rows INTEGER NOT NULL, failed_rows INTEGER NOT NULL, detail TEXT, imported_at INTEGER NOT NULL);
-- 版本管理交给 sqlx _sqlx_migrations；每次升级只允许新增迁移文件，禁止改写已发布迁移
```

---

## 三、任务拆解（按依赖波次）

> 编号即工单号。每个工单都写明**独占写入目录**，这是并行不冲突的唯一保证。

### Wave 0 — 基座与契约（串行，仅 1 个 agent）

| 工单 | 内容 | 独占写入范围 | 验收 |
|---|---|---|---|
| **T0** 基线 | 旧数据目录排查 + 备份 + 旧数据格式说明；建议主控先 `git init` 建基线（需用户同意）；创建 workspace、`rust-toolchain.toml`、`.cargo/config.toml`、`.gitignore`；预置**全部**依赖到各 crate 的 `Cargo.toml`（后续 agent 不得新增依赖）；搭 Tauri 2 + Vite/React/TS 空壳 | 根 `Cargo.toml`、`Cargo.lock`、`crates/*/Cargo.toml`、`apps/desktop` 与 `apps/desktop/src-tauri` 骨架、`docs/` | `cargo check -j 1` 全绿；`npm run build` 空壳可过 |
| **T1** 领域模型 | C1 全部内容 + 单元测试 | `crates/core/**` | `cargo test -p umt-core -j 1` |
| **T2** 协议接口 | C2 全部内容 + 一个 `MockProvider` 供其他 agent 写测试 | `crates/protocol-api/**` | 同上 |
| **T3** 存储契约 | C3 建表 SQL、迁移 v1、仓库 trait + `InMemory*Repo` 假实现（供并行使用） | `crates/persistence/{src/schema.sql,src/repos/**,migrations/**}` | 同上 |
| **T4** 安全契约 | C4；确定密钥存放方案（Windows DPAPI/凭据管理器 vs 文件+权限），写进 `docs/security/key-storage.md` | `crates/security/**`、`docs/security/**` | 同上 |
| **T5** IPC 契约 | C5：Tauri 命令清单、DTO、错误码、前端 `types.ts`/`client.ts` 桩 | `crates/app/src/ipc/**`、`apps/desktop/src/api/**` | `cargo check -p umt-app -j 1` + `npm run typecheck` |

**T0–T5 全部完成后才允许开 Wave 1**。此时契约文件打上 `// CONTRACT-FROZEN: <hash>` 标记。

### Wave 1 — 服务层与三个 Provider（6 个 agent 并行）

| 工单 | 内容 | 依赖 | 独占写入 | 验收 |
|---|---|---|---|---|
| **T6** 持久化实现 | sqlx 实现全部 Repo、连接池、迁移执行、启动恢复、`revision` 乐观锁 | T1,T3 | `crates/persistence/src/{pool,db,repos/impl_*}.rs` | 重启恢复集成测试：写入→关闭→重开→数据一致 |
| **T7** SSH Provider | russh：密码/私钥认证、Host Key 校验、keepalive、exec、交互 shell、resize、超时 | T2 | `crates/providers/ssh/**` | 对本地 WSL sshd 或 Docker sshd 的真连测试；错误分类测试 |
| **T8** Telnet Provider | 提示符状态机（login/password/prompt）、分页 `--More--`、超时、断线检测 | T2 | `crates/providers/telnet/**` | 用 tokio 假设备做交互测试 + 一台真实设备手测 |
| **T9** WSL Provider | `wsl.exe -d <distro>` 子进程、发行版枚举、exec、交互 shell、进程回收（Job Object 防泄漏） | T2 | `crates/providers/wsl/**` | 本机实测 + 关闭后无残留进程断言 |
| **T10** 访问控制 | Token 生成/哈希/校验、有效期、启用禁用撤销、M2M 授权、每请求七项校验、撤销联动 Session 终止 | T3(假实现),T4 | `crates/access-control/**` | 权限矩阵单测（含"改 connection_id 越权"用例） |
| **T11** Session 管理器 | Registry 查询、会话生命周期状态机、环形输出缓冲 + 游标读取、超时回收、Token 绑定、并发上限 | T2,T10(接口) | `crates/session-manager/**` | 状态机单测 + 超时回收测试 |
| **T12** 审计 | 会话/命令/拒绝事件写入、敏感信息脱敏、超时 vs 断开 vs 失败三态区分、筛选查询、导出 | T3(假实现) | `crates/audit/**` | 脱敏用例 + 状态区分用例 |

并行要点：T6 与 T10/T12 之间只通过 **trait** 交互，T10/T12 用 T3 的 `InMemory*Repo` 完成单测，等 T6 合并后再补一条真实 DB 集成测试。

### Wave 2 — 对外接口与 UI（6 个 agent 并行）

| 工单 | 内容 | 依赖 | 独占写入 | 验收 |
|---|---|---|---|---|
| **T13** MCP Server | rmcp streamable-http、Bearer 中间件、`127.0.0.1:17321/mcp`、端口占用检测、7 个工具、能力校验、结构化结果 | T10,T11,T12 | `crates/mcp-server/**` | 用官方 MCP 客户端（或 `mcp-inspector`）跑通 7 个工具；未认证/过期/越权三类拒绝用例 |
| **T14** App 层接线 | Tauri commands 实现 T5 契约、启动初始化顺序（DB→迁移→Registry→Provider 注册→MCP 自动启动策略）、日志、托管状态 | T5,T6,T10,T11,T12,T13 | `crates/app/src/{commands,state,setup}.rs`、`apps/desktop/src-tauri` | `cargo tauri dev` 可启动，前端可拿到真实数据 |
| **T15** 前端基座 | 路由、布局、设计系统、`SchemaForm` 动态表单渲染器、统一请求封装与错误提示 | T5 | `apps/desktop/src/{layout,components,lib}/**` | typecheck/lint/test/build 全绿；SchemaForm 单测覆盖 5 种字段类型 |
| **T16** 连接管理页 | 列表/筛选/创建/编辑/删除/复制/测试/启停/详情；表单完全由 Schema 驱动 | T15 | `apps/desktop/src/pages/connections/**` | 手工验收清单 + 组件测试 |
| **T17** 访问控制页 | Token 列表、创建一次性展示、复制、脱敏、状态操作、详情页授权开关矩阵 | T15 | `apps/desktop/src/pages/access-control/**` | 同上 |
| **T18** 审计日志页 | Session 列表、命令时间线、筛选、详情输出、导出 | T15 | `apps/desktop/src/pages/audit/**` | 同上 |
| **T19** 首页 + 设置 | 5 步快速开始、MCP 状态卡、JSON 配置生成、AI 提示词复制（不含真实 Token）、服务启停/端口/超时设置 | T15 | `apps/desktop/src/pages/{home,settings}/**` | 同上 |

### Wave 3 — 集成与验收（2 个 agent 并行）

| 工单 | 内容 | 降级要求 |
|---|---|---|
| **T20** 集成测试 | 重启恢复、权限矩阵、Token 撤销终止会话、MCP 端到端、Telnet/WSL/SSH 三协议冒烟 | 全部对应需求 §15 验收项，逐条给结论 |
| **T21** 安全加固与文档 | 日志泄露扫描、`cargo audit`、密码/私钥/Toke 明文检查、架构/MCP/协议开发文档 | 文档：`docs/architecture/`、`docs/mcp/`、`docs/protocols/`、`docs/security/` |

---

## 四、并行冲突控制规则

1. **单写者原则**：一个工单只写自己表格里的路径。需要改别人的文件 → 提交"契约变更请求"给主控，主控串行修改并通知受影响 agent。
2. **依赖只由 T0 添加**：任何新增 crate 依赖必须先由主控加到 `Cargo.toml`，否则 `Cargo.lock` 并行冲突必现。
3. **migrations 唯一写者 = T6**：其他工单不得新增迁移文件，只能提需求。
4. **共享契约改动必须一次到位**：`crates/core`、`protocol-api`、`persistence/src/repos`、`apps/desktop/src/api/types.ts` 冻结后只读。
5. **并发上限 4–6 个 agent**：按项目记忆的教训，编译需 `-j 1`；建议同时最多 4 个 agent 编译，其余只做读/写不编译，由主控批量跑门禁。
6. **每个 agent 交付前必须自检**：
   `cargo fmt --all -- --check` → `cargo check -j 1 -p <crate>` → `cargo clippy -j 1 -p <crate> -- -D warnings` → `cargo test -j 1 -p <crate>`；前端工单：`npm run typecheck && npm run lint && npm run test -- --run && npm run build`。
7. **禁止的事**：模拟连接成功、前端假数据、在核心层写协议分支、在 UI 硬编码协议字段、只在前端做权限判断、记录明文凭据、擅自 commit/push。

---

## 五、Agent 提示词模板（复制即用）

```text
项目：Universal MCP Terminal（Rust workspace + Tauri 2 + React/TS）
必读：docs/requirements/universal-mcp-terminal.md 全文、
      docs/development/multi-agent-execution-plan.md 第二、四节。
你负责工单 <TICKET_ID>：<一句话目标>。

约束：
- 只允许写入：<owned paths>。其他文件一律只读；确需修改，停下并输出「契约变更请求」。
- 契约文件已冻结（crates/core、protocol-api、persistence/repos、security、app/ipc、apps/desktop/src/api/types.ts）。
- 不得新增第三方依赖（依赖已由 T0 预置）。
- 严禁模拟成功、假数据、硬编码凭据。
- 编译/测试一律加 -j 1。

交付：
1. 代码 + 单元测试（覆盖表格中的验收点）。
2. 自检结果：fmt / check / clippy -D warnings / test 的真实输出摘要。
3. 300 字内说明：做了什么、遗留问题、对其他工单的影响。
```

---

## 六、风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 契约设计不足导致 Wave 1 返工 | 高 | T0–T5 视为一次性投入，必要时先用 MockProvider/InMemoryRepo 验证接口可用性再冻结 |
| SSH 无可用测试目标 | 中 | 优先用本机 WSL sshd；不可用时 Docker 起 sshd；再不可用则标记"需人工验收" |
| Telnet 提示符千差万别 | 高 | 提示符做成可配置正则 + 状态机，并为常见设备（Cisco/Huawei/H3C）留样例测试 |
| Host Key 无人值守场景无解 | 中 | 默认拒绝 + 要求人工在 UI 确认，写进安全文档与 MCP 错误码 |
| Secret 密钥存哪里 | 高 | T4 明确方案并落文档；禁止"密钥与密文同目录明文存放" |
| 多 agent 同时编译 OOM | 中 | `-j 1` + 最多 4 个 agent 并发编译 + 主控批量验收 |
| 无 Git 导致无法回退 | 高 | 建议用户同意后 `git init` + 每波结束打基线标签 |

---

## 七、执行顺序总览

```text
Wave 0 (串行): T0 → T1 → T2 → T3 → T4 → T5   [契约冻结]
Wave 1 (并行): T6 | T7 | T8 | T9 | T10 | T11 | T12
Wave 2 (并行): T13 | T14 | T15 → (T16 | T17 | T18 | T19)
Wave 3 (并行): T20 | T21
每波结束：主控统一跑全量门禁 + 验收表 + 更新文档
```

---

## 八、执行进度（滚动更新）

### T0 已完成（2026-11-12）

| 交付项 | 结果 |
|---|---|
| Git 基线 | 已 `git init`，`backup/`、`target/`、`node_modules/`、`dist/` 已忽略 |
| Rust workspace | 14 个 crate 骨架全部创建，依赖统一收敛到 `[workspace.dependencies]` |
| 依赖锁定 | serde 1.0.229 / tokio 1.54 / sqlx 0.9 / rmcp 3.9 / russh 0.65 / tauri 2.12 / keyring 4.2 / aes-gcm 0.11 |
| Tauri 外壳 | `apps/desktop/src-tauri` 可编译，图标已生成，capabilities 已配置 |
| 前端空壳 | `apps/desktop`：React 19 + Vite 8 + TS 6 + Vitest，5 个路由页面占位 + 侧边栏布局 |
| 旧数据 | 已备份到 `backup/legacy-data-20261112/` 并完成盘点，见 [legacy-data-inventory.md](../requirements/legacy-data-inventory.md) |
| 门禁 | `cargo fmt --check` / `cargo check` / `cargo clippy -D warnings` / `cargo test` 全通过；前端 typecheck / lint / test / build 全通过 |

**环境约束（后续 agent 必须知道）**

1. 本机没有 MSVC 构建工具，`rust-toolchain.toml` 固定为 `stable-x86_64-pc-windows-gnu`，不要改回默认 `stable`。
2. 编译统一 `-j 1`（`.cargo/config.toml` 已设 `build.jobs = 1`）。
3. `keyring` 只启用 `windows-native-keyring-store` 特性，新增其他平台特性需主控确认。
4. 前端产物固定在 `apps/desktop/dist`，Tauri `frontendDist` 指向 `../dist`。
5. 不要删除 `backup/` 下的旧数据备份。
