# Universal MCP Terminal

面向本地 AI Agent 的通用终端与远程设备连接管理平台。软件本身不内置 AI 模型，
而是通过内置的 MCP Server 把用户明确授权的连接安全地提供给 Claude Code、Codex、
Cursor、OpenCode 等支持 MCP 的客户端。

核心原则：**协议可无限扩展、AI 只能访问被授权的连接、所有操作都被审计。**

## 能力概览

- **可插拔协议**：SSH / Telnet / WSL 通过统一 `ProtocolProvider` 接入，核心层无协议分支
- **连接管理**：配置由 Provider 的 JSON Schema 动态生成表单，凭据 AES-256-GCM 加密存储
- **Token 与授权**：256 bit Token（只存 HMAC 与前缀）、Token ↔ 连接多对多授权、撤销即时生效
- **MCP Server**：Streamable HTTP + Bearer，9 个工具，HTTP 层与工具层双重鉴权
- **会话与审计**：会话状态机、空闲回收、命令/输入时间线、敏感信息脱敏、CSV 导出
- **安全执行策略**：ReadOnly / Restricted / FullAccess，高风险命令识别与拒绝
- **安全细节**：SSH Host Key 三态校验与人工确认、Token 常量时间比较、凭据不落明文

## 快速开始（开发）

前置：Windows、Rust stable（GNU 工具链，见 `rust-toolchain.toml`）、Node 22、WebView2。

```powershell
# 前端依赖与构建
cd apps/desktop
npm ci
npm run build

# 启动桌面应用（会同时启动 Vite dev server）
npm run tauri dev
```

首次启动会在 `%LOCALAPPDATA%\UniversalMCPTerminal\` 创建 `umt-v2.db`，并把主密钥写入
Windows 凭据管理器（service：`com.universal-mcp-terminal`）。

## MCP 接入

默认地址：`http://127.0.0.1:17321/mcp`（Streamable HTTP，仅监听回环地址）。

```json
{
  "mcpServers": {
    "universal-mcp-terminal": {
      "transport": "streamable-http",
      "url": "http://127.0.0.1:17321/mcp",
      "headers": {
        "Authorization": "Bearer YOUR_ACCESS_TOKEN"
      }
    }
  }
}
```

Token 在桌面端「访问控制」页创建，只显示一次；随后在 Token 详情里开启需要授权的连接。
完整工具列表与排错说明见 [MCP 接入指南](docs/mcp/接入指南.md)。

## 质量门禁

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -j 1 -- -D warnings
cargo test --workspace -j 1
cd apps/desktop; npm run typecheck; npm run lint; npm run test; npm run build
```

当前基线：Rust 182 个测试通过（1 个真实 SSH 连接用例需环境变量），前端 66 个测试通过；
`tauri build --debug --no-bundle` 已验证可产出桌面可执行文件。
详见 [测试报告](docs/development/test-report.md)，CI 配置见 `.github/workflows/ci.yml`。

## 目录结构

```text
crates/
  core/              领域模型与统一错误
  protocol-api/      Provider / Session 抽象与配置 Schema
  protocol-registry/ 协议注册中心
  providers/         ssh / telnet / wsl 三个独立实现
  persistence/       SQLite 迁移、仓库契约与实现
  security/          AES-GCM、Token HMAC、Host Key 策略
  access-control/    Token 生命周期与授权校验
  session-manager/   会话状态机、执行策略与回收
  audit/             脱敏、审计写入与导出
  mcp-server/        MCP 工具与 HTTP 服务
  app/               应用装配、IPC 命令与服务层
apps/desktop/        Tauri 2 应用 + React 前端
docs/                需求、架构、协议、MCP、安全与测试文档
```

## 文档

- [架构总览](docs/architecture/overview.md)
- [协议开发指南](docs/protocols/provider-development.md)
- [MCP 接入指南](docs/mcp/接入指南.md)
- [密钥与凭据存储](docs/security/key-storage.md)
- [测试报告](docs/development/test-report.md)
- [真实设备联调记录](docs/development/real-device-integration.md)
- [需求说明](docs/requirements/universal-mcp-terminal.md)

## 已知限制

- 首期不支持文件传输（FTP/SFTP 等），也不提供终端录制回放
- MCP 通道暂不提供交互式高风险确认，`FullAccess + 需确认` 会按拒绝处理
- Telnet 未实现完整选项协商，真实设备提示符需按需调整正则
- WSL 能力仅限 Windows
