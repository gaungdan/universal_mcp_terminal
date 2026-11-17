# AGENT.md

给接手本仓库的 AI Agent 的速查手册。**先读本文件，再读 `docs/architecture/overview.md`。**

## 1. 项目是什么

Universal MCP Terminal 是一个 Windows 桌面应用（Tauri 2 + React 19），本身**不含 AI 模型**。
它把用户明确授权的 SSH / Telnet / WSL 连接，通过内置 MCP Server（Streamable HTTP + Bearer）
安全地暴露给 Claude Code、Codex、Cursor 等 MCP 客户端。

三条不可违背的产品原则：

1. **协议可无限扩展**：新增协议只写一个 Provider crate，核心层不出现 `match protocol_id`。
2. **AI 只能访问被授权的连接**：Token ↔ 连接多对多授权，鉴权只从 HTTP Header 取，绝不从工具参数取。
3. **所有操作都被审计**：会话与命令全量落库，敏感信息脱敏，不可伪造成功。

## 2. 快速开始

前置：Windows、Rust `stable-x86_64-pc-windows-gnu`（见 `rust-toolchain.toml`，本机没有 MSVC）、Node 22、WebView2。

```powershell
cd apps/desktop
npm ci
npm run tauri dev        # 同时拉起 Vite(127.0.0.1:5173) 与 Tauri 窗口
```

首次启动创建 `%LOCALAPPDATA%\UniversalMCPTerminal\umt-v2.db`，主密钥写入 Windows 凭据管理器
（service `com.universal-mcp-terminal`）。旧版 `umt.db` 一律不读取。

**编译硬约束**：`.cargo/config.toml` 固定 `jobs = 1`（本机内存有限，多并发会 OOM）。
手工执行 cargo 时也请带 `-j 1`，不要为了快而并行编译。

## 3. 本机环境已知坑（务必先确认）

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `program not found: cargo` | rustup 重装后用户 PATH 丢了 `%USERPROFILE%\.cargo\bin` | 临时：`$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"`；长期：加进用户环境变量 |
| `WebView2 error ... HRESULT(0x80070002)` | `EdgeWebView\Application\<ver>\msedgewebview2.exe` 被删（本机 Avira 隔离） | 管理员重装 WebView2 Runtime 并给杀软加白；临时可用固定版本运行时 + `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` 指向它 |
| Tauri 窗口全白 | Vite 只绑 `::1`，WebView2 走 IPv4 | 已在 `vite.config.ts` 固定 `host: "127.0.0.1"`，`devUrl` 同步为 `http://127.0.0.1:5173` |

## 4. 目录与职责

```text
crates/
  core/               领域模型（Id 类型、UmtError、事件）——无任何内部依赖
  protocol-api/       ProtocolProvider / TerminalSession / ConfigSchema / 能力集
  protocol-registry/  协议注册中心（按 protocol_id 取 Provider）
  providers/{ssh,telnet,wsl}/  协议实现，只依赖 core + protocol-api
  persistence/        sqlx SQLite、迁移、Repo trait 与实现
  security/           AES-256-GCM、Token HMAC、Host Key 策略、KeyStore
  access-control/     Token 生命周期、授权校验、SessionRevoker trait
  session-manager/    会话状态机、执行策略（ReadOnly/Restricted/FullAccess）、空闲回收
  audit/              脱敏、审计写入、CSV 导出
  mcp-server/         rmcp Streamable HTTP、Bearer 中间件、9 个工具
  app/                装配（AppState）、services 层、33 个 IPC 命令
apps/desktop/src-tauri/  Tauri 壳（只做装配与 invoke_handler 注册）
apps/desktop/src/        React 前端：pages/{home,connections,access-control,audit,settings}
```

**分层约束（破坏即架构退化，PR 不予合并）**

- `umt-core`、`umt-protocol-api` 不依赖 tokio / 网络 / 数据库 / 具体协议。
- Provider crate 不依赖 persistence / audit / access-control / session-manager。
- `mcp-server`、`session-manager`、`access-control`、`audit` 都不 `use umt_provider_*`。
- `access-control` 不反向依赖 `session-manager`，撤销联动走 `SessionRevoker` 注入。
- 只有 `umt-app` 同时认识所有 Provider，注册点在 `AppState::initialize_with_keystore`。

## 5. 契约层：改动顺序

先冻结、后实现。以下文件属于**跨 crate 契约**，改它们要同步全链路：

| 契约 | 位置 | 同步对象 |
| --- | --- | --- |
| 领域模型 / 错误 / 事件 | `crates/core/src/**` | 所有 crate |
| Provider 与 Session 抽象 | `crates/protocol-api/src/**` | 3 个 Provider、session-manager、mcp-server |
| 仓库 trait 与表结构 | `crates/persistence/src/**` + `migrations/*.sql` | access-control、session-manager、audit、app |
| 安全原语 | `crates/security/src/**` | access-control、providers、app |
| IPC DTO | `crates/app/src/ipc/dto.rs` | `apps/desktop/src/api/types.ts`、`client.ts` |

迁移只允许**新增** `000N_xxx.sql`，禁止改写已发布的迁移文件。
DTO 与 `types.ts` 必须字段同名同义，前端不做二次映射。

## 6. 数据与密钥

- 表：`connections`、`connection_secrets`、`tokens`、`token_connection_grants`、`sessions`、`audit_events`、`settings`、`ssh_host_keys`。
- 设置键：`mcp.host`（默认 `127.0.0.1`）、`mcp.port`（默认 **47321**）、`security.default_execution_mode`、`security.session_timeout_secs`（默认 1800）、`security.require_confirmation_for_high_risk`、`log.level`。
- 凭据：AES-256-GCM，AAD 固定 `umt-v1`，密文存 `connection_secrets`；敏感字段**永不**写入 `connections.config_json` 或日志。
- Token：`umt_<8 位 hex 前缀>_<64 位 hex>`（256 bit 熵）。库里只存前缀与 HMAC-SHA256(pepper)，比较用常量时间。明文只在创建时返回一次。
- 端口变更时注意 `seed_defaults` 里的旧值迁移逻辑（17321/17322 → 47321）。

## 7. MCP Server

- 地址：`http://127.0.0.1:47321/mcp`，仅监听回环；代码常量在 `crates/mcp-server/src/lib.rs`。
- 鉴权链路：HTTP 层 Bearer 中间件 → `AccessControl::authenticate`（HMAC + 常量时间比较）→ 每个工具再做 `authorize_connection`；涉及会话的调用由 `SessionManager` 校验归属。
- 10 个工具：`list_connections`、`get_connection_info`、`test_connection`、`open_session`、`execute_command`、`read_file`、`send_input`、`read_output`、`resize_terminal`、`close_session`（`TOOL_NAMES` 是白名单，新增工具要同步 `tool_definitions` 与文档）。
- Token 权限只有 `ReadOnly`（默认）与 `WriteAccess`；`ReadOnly` 使用允许清单并拒绝未知命令，`WriteAccess` 允许普通写操作但拦截灾难命令与提权命令。
- 交互式终端：`ReadOnly` 永远禁止；`WriteAccess` 还需 `allow_interactive=true`。交互式输入等同于完整 Shell，不做逐条命令过滤。
- `execute_command`、`open_session`、`send_input` 都必须经过 `PermissionGate` 与 `SessionManager` 的双重校验。
- 不支持的能力必须返回 `UnsupportedCapability`，不允许假装成功。
- 日志只记录错误码与脱敏文本。

## 8. 安全不变量

1. Token 只从 `Authorization` 头解析，任何工具参数里的 token 一律忽略。
2. 命令权限以 Token 为准，不再读取全局执行模式；`sudo`/`su` 等提权命令对所有 Token 一律拒绝（尽力而为，最终依赖连接账号权限）。
3. SSH 未知 Host Key 默认拒绝；信任需人工确认并落 `ssh_host_keys`。
4. 撤销 Token / 禁用连接对已建立会话立即生效。
5. 审计记录不可被业务逻辑跳过；拒绝也要记 `denied` 原因。

## 9. 质量门禁

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -j 1 -- -D warnings
cargo test --workspace -j 1

cd apps/desktop
npm run typecheck
npm run lint
npm run test
npm run build
```

基线（改动前请先跑一遍确认没有既有失败）：Rust 182 passed / 2 ignored，前端 66 passed，
`cargo clippy -D warnings` 全绿。CI 见 `.github/workflows/ci.yml`（Windows + MSVC，WSL Provider 用例不阻塞）。

**真实设备测试（默认 `#[ignore]`）**

```powershell
# SSH（WSL sshd 或真机）
$env:UMT_TEST_SSH_HOST="172.27.39.33"; $env:UMT_TEST_SSH_USER="guang"
$env:UMT_TEST_SSH_PRIVATE_KEY="d:\path\id_ed25519"
cargo test -p umt-provider-ssh -- --ignored

# Telnet：先起模拟器，再跑
python scripts/telnet_device_sim.py --port 2323
$env:UMT_TEST_TELNET_HOST="127.0.0.1"; $env:UMT_TEST_TELNET_PORT="2323"
$env:UMT_TEST_TELNET_USER="admin"; $env:UMT_TEST_TELNET_PASSWORD="admin"
cargo test -p umt-provider-telnet -- --ignored
```

WSL Provider 用例需要本机真实 WSL 发行版，CI 上跳过属预期。

## 10. 代码与提交约定

- Rust edition 2024，注释与错误文案用中文；`#![deny(unsafe_code)]` 已在库 crate 开启。
- 新增 Provider 请照 `docs/protocols/provider-development.md` 的清单走，并补单测 + 契约测试。
- 前端不直连数据库、不自己判断权限；所有判定在后端。
- 提交信息用中文 `type: 摘要`（feat/fix/chore/docs/refactor/test），一次提交只做一件事。
- 改完必须跑第 9 节门禁再提交，不要用 `--no-verify`。

## 11. 已知限制

- 支持 SSH/SFTP 与 WSL 的只读文件下载；无 FTP/TFTP、无文件上传、无终端录制回放。
- MCP 通道无交互式高风险确认。
- Telnet 未实现完整选项协商，提示符正则按设备调整。
- WSL 能力仅 Windows。
- 未提供安装包签名与自动更新。

## 12. 关键文件索引

| 想做什么 | 从这里开始 |
| --- | --- |
| 理解一次 MCP 调用全链路 | `docs/architecture/overview.md` 第二节 |
| 新增/改协议 | `crates/protocol-api/src/lib.rs`、`docs/protocols/provider-development.md` |
| 改数据库 | `crates/persistence/migrations/`、`crates/persistence/src/repo/` |
| 改鉴权 | `crates/access-control/src/`、`crates/mcp-server/src/handler.rs` |
| 改执行策略 | `crates/session-manager/src/policy.rs` |
| 改 IPC 接口 | `crates/app/src/ipc/commands.rs` + `dto.rs` + `apps/desktop/src/api/types.ts` |
| 改应用装配/默认设置 | `crates/app/src/services/state.rs` |
| 改界面 | `apps/desktop/src/pages/*`、`components/SchemaForm.tsx` |
