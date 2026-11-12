# Universal MCP Terminal — 完整项目重构与功能开发需求

## 一、项目基本信息

**项目名称：** Universal MCP Terminal

**项目标识：** `universal_mcp_terminal`

**项目定位：**

Universal MCP Terminal 是一个面向本地 AI Agent 的通用终端与远程设备连接管理平台。

软件本身不内置 AI 模型，而是通过本地 MCP Server 向不同 AI 客户端提供统一的设备连接、终端操作和会话管理能力。

整体体验可以参考 MobaXterm，但核心使用场景不是传统人工终端管理，而是让本地 AI 工具能够安全、统一地访问用户授权的远程设备和本地运行环境。

支持接入：

- Claude Code
- Codex
- Cursor
- OpenCode
- 其他支持 MCP 协议的 AI 客户端

**核心设计理念：**

Universal Connectivity + Pluggable Protocols + MCP + Security + Audit

本项目最重要的架构要求是：协议必须具备高度可扩展性，不能将具体协议实现与核心业务逻辑耦合。

---

# 二、首期协议支持范围

首期仅实现以下三种协议：

| 协议 | Provider | 主要用途 |
|---|---|---|
| SSH | SSH Provider | Linux、Unix、网络设备远程连接 |
| Telnet | Telnet Provider | 老式路由器、交换机等网络设备 |
| WSL | WSL Provider | Windows 本地 Linux 子系统 |

明确不实现：

- FTP
- FTPS
- SFTP

不保留上述协议的 Provider、配置表单、依赖和 MCP 工具适配。

首期不实现远程文件传输功能。

但必须保留通用协议能力抽象，不能因为当前只有三种协议就把整个架构写死。

未来可以独立扩展：

- Serial / RS232 / RS485
- RDP
- VNC
- SMB
- SNMP
- Docker
- Kubernetes
- HTTP / HTTPS API
- WebSocket
- 数据库连接
- 自定义 TCP / UDP
- 其他网络设备管理协议

新增协议时，应尽可能只需要增加新的 Provider，不需要修改 Token 管理、MCP Server、Session Manager 和审计系统。

---

# 三、项目现状与重构要求

当前项目的数据恢复功能无法正常恢复数据。

本次不再继续无休止地修补原有数据恢复逻辑。

首先检查：

1. 当前项目的数据持久化方式。
2. 数据存储位置。
3. 数据序列化和反序列化逻辑。
4. 数据库结构及迁移机制。
5. 数据读取和恢复流程。
6. 软件重启后的初始化流程。
7. 前端状态与后端实际数据是否一致。

如果原有实现存在结构性问题，可以直接重写相关模块。

要求：

- 连接数据可以持久化。
- Token 数据可以持久化。
- Token 与连接授权关系可以持久化。
- Session 和审计日志可以持久化。
- 软件重启后能够正确恢复。
- 数据模型必须具有明确的版本管理机制。
- 后续升级不能轻易破坏已有用户数据。

不要只修复前端显示问题，必须保证底层数据真实可靠。

对于可能造成已有用户数据丢失的迁移、删除或重置操作，先制定备份和迁移方案，并向我确认。

---

# 四、最高优先级：可插拔协议架构

这是整个项目最重要的架构要求。

禁止在核心业务层直接编写 SSH、Telnet、WSL 等具体协议的专属逻辑。

所有协议必须通过统一的 Provider / Adapter 接口接入。

## 4.1 整体架构

建议分为以下模块：

1. Core：核心业务与统一抽象。
2. Protocol API：协议 Provider 接口。
3. Protocol Registry：协议注册中心。
4. Protocol Providers：各协议独立实现。
5. Session Manager：统一会话管理。
6. MCP Server：对 AI 暴露标准化工具。
7. Access Control：Token 和连接权限控制。
8. Audit：操作审计。
9. Persistence：数据持久化。
10. UI：连接管理、访问控制、审计展示和系统设置。

核心业务模块只能依赖统一抽象，不能直接依赖具体协议实现。

## 4.2 Provider 统一接口

建议使用 Rust Trait 实现协议抽象。

每个 Provider 必须能够提供：

- 唯一协议 ID。
- 协议名称。
- 协议版本。
- 协议描述。
- 配置 Schema。
- 支持的能力列表。
- 配置验证。
- 连接测试。
- 建立连接。
- 创建 Session。
- 发送数据。
- 接收数据。
- 关闭 Session。
- 资源释放。

可参考以下抽象：

```rust
#[async_trait]
pub trait ProtocolProvider: Send + Sync {
    fn protocol_id(&self) -> &str;

    fn display_name(&self) -> &str;

    fn capabilities(&self) -> Vec<Capability>;

    fn config_schema(&self) -> ConfigSchema;

    async fn validate_config(
        &self,
        config: &ConnectionConfig,
    ) -> Result<(), ProviderError>;

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError>;

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn TerminalSession>, ProviderError>;
}
```

以上只是架构参考，实际实现时需要根据异步 Session、对象生命周期、可变状态和资源释放机制进行合理设计。

不要为了追求接口统一而强迫所有协议实现不适用的功能。

## 4.3 Capability 能力声明

协议需要动态声明支持的能力。

建议预留：

- Shell
- Exec
- InteractiveTerminal
- DeviceInformation
- PortForward
- TerminalResize
- EnvironmentInfo

首期协议能力：

| 能力 | SSH | Telnet | WSL |
|---|---|---|---|
| Shell | 支持 | 支持 | 支持 |
| Exec | 支持 | 支持 | 支持 |
| InteractiveTerminal | 支持 | 支持 | 支持 |
| TerminalResize | 支持 | 视实现情况 | 支持 |
| DeviceInformation | 可扩展 | 可扩展 | 可扩展 |
| PortForward | 可扩展 | 不支持 | 不支持 |

不同 Provider 只能声明自己真实支持的能力。

MCP Server 根据连接的 Capability 决定可以提供哪些操作。

不支持的能力必须明确返回错误，不能模拟成功。

## 4.4 动态协议注册

实现 Protocol Registry。

应用启动时注册已经实现的 Provider。

核心系统通过 Registry 查询可用协议。

UI 根据 Provider 返回的配置 Schema 动态生成表单。

禁止在 UI 中大量使用：

```typescript
if (protocol === "SSH") {
    // ...
} else if (protocol === "TELNET") {
    // ...
}
```

协议的配置字段、默认值、校验规则应由对应 Provider 提供。

新增协议时，只需要：

1. 新建 Provider。
2. 实现统一接口。
3. 声明能力。
4. 提供配置 Schema。
5. 注册 Provider。
6. 编写协议测试。

不要因为增加协议而修改核心业务模块。

## 4.5 插件化的长期扩展

首期采用 Rust Trait + Registry + 独立 Crate。

不要为了第一版就引入复杂的动态 Rust 动态库加载机制。

未来如果需要第三方独立开发 Provider，可以进一步支持：

- 动态库插件。
- 独立进程插件。
- 插件元数据。
- 插件版本兼容。
- 插件启用和禁用。
- 插件权限隔离。

插件必须运行在受控环境中，不能因为支持插件就允许任意代码直接访问所有用户凭据。

---

# 五、连接管理

## 5.1 连接创建

在连接管理页面提供创建连接功能。

用户可以选择协议类型：

- SSH
- Telnet
- WSL

根据选择的 Provider 动态展示对应配置表单。

通用字段：

- 连接名称。
- 协议类型。
- 连接描述。
- 标签。
- 备注。
- 创建时间。
- 更新时间。
- 启用状态。

## 5.2 SSH 配置

支持：

- Host。
- Port，默认 22。
- Username。
- Password。
- Private Key。
- Private Key Passphrase。
- Authentication Type。
- Connection Timeout。
- Keep Alive。
- Terminal Type。
- 默认工作目录。

认证方式支持：

- Password Authentication。
- Public Key Authentication。

密码和私钥不能明文存储。

SSH 连接需要正确验证服务端 Host Key。

首次连接遇到未知 Host Key 时，应提示用户确认并保存指纹。

后续指纹变化必须触发安全警告，不能默认忽略校验。

## 5.3 Telnet 配置

支持：

- Host。
- Port，默认 23。
- Username。
- Password。
- Connection Timeout。
- Login Prompt。
- Password Prompt。
- Terminal Type。

需要考虑不同网络设备的登录交互方式。

支持：

- 用户名提示识别。
- 密码提示识别。
- 登录成功判断。
- 命令提示符识别。
- 分页处理。
- 连接超时。
- 异常断开。

Telnet 不提供加密，必须明确提示用户仅在可信网络中使用。

## 5.4 WSL 配置

WSL 是本地运行环境，不是网络协议。

通过 Windows 的 `wsl.exe` 调用 WSL。

支持：

- Distribution Name。
- 默认工作目录。
- Shell 类型。
- 启动参数。
- 进程超时。
- 终端环境变量。

例如：

```powershell
wsl.exe -d Ubuntu -- bash -lc "uname -a"
```

支持多个 WSL Distribution。

应能够读取本机已经安装的发行版，并允许用户选择。

WSL Provider 需要正确管理子进程生命周期，避免进程泄漏。

同时明确区分 Windows 本地进程和 WSL 子进程的权限边界。

## 5.5 连接测试

创建连接时提供「测试连接」按钮。

流程：

1. 用户填写配置。
2. 点击测试连接。
3. 后端找到对应 Provider。
4. Provider 验证配置。
5. 尝试建立真实连接。
6. 完成实际认证。
7. 返回测试结果。
8. 释放测试资源。

不能只检查 TCP 端口是否开放。

成功信息：

- SSH 认证成功。
- Telnet 登录成功。
- WSL 环境启动成功。

失败时展示：

- 连接失败原因。
- 错误类型。
- 目标地址。
- 建议排查方向。

测试连接不能将真实密码、私钥写入日志。

## 5.6 连接管理操作

支持：

- 创建连接。
- 编辑连接。
- 删除连接。
- 复制连接。
- 测试连接。
- 查看连接详情。
- 启用连接。
- 禁用连接。
- 查看连接授权状态。

连接数据必须持久化。

连接被禁用后，MCP 不得再创建新的 Session。

---

# 六、访问控制与 Token 管理

这是本次需求的核心模块。

## 6.1 Token 创建

在访问控制页面增加 Token 管理。

用户可以创建多个 Token。

Token 配置：

- Token 名称。
- Token 描述。
- 有效期。
- 创建时间。
- 最后使用时间。
- Token 状态。
- Token 值。

Token 使用安全随机数生成，必须具有足够的熵。

建议使用至少 256 bit 的随机 Token。

创建成功后：

- 展示完整 Token。
- 提供复制按钮。
- 明确提示用户妥善保存。
- 后续仅展示脱敏值。

Token 不应明文持久化。

建议使用带密钥的 HMAC-SHA256 对 Token 进行校验存储，使用常量时间比较。

支持：

- 创建 Token。
- 查看 Token 信息。
- 启用 Token。
- 禁用 Token。
- 撤销 Token。
- 删除 Token。
- 查看最后使用时间。

## 6.2 Token 与连接授权绑定

一个 Token 可以管理多个连接。

在 Token 详情页面展示全部已配置连接。

每条连接提供独立授权开关。

例如：

Token：Claude Code

| 连接名称 | 协议 | 授权状态 |
|---|---|---|
| Production Linux | SSH | 开启 |
| Development Linux | SSH | 开启 |
| Home Router | Telnet | 关闭 |
| Ubuntu WSL | WSL | 开启 |

授权逻辑：

- 开启：允许当前 Token 访问该连接。
- 关闭：禁止当前 Token 访问该连接。

必须满足：

1. 新建 Token 默认不授权任何连接。
2. 新建连接默认不授权任何 Token。
3. 一个连接可以授权给多个 Token。
4. 一个 Token 可以访问多个连接。
5. 关闭授权后，权限立即失效。
6. 禁用 Token 后，Token 立即失效。
7. 撤销 Token 后，所有连接授权立即失效。
8. 已有 Session 的处理必须明确：权限撤销后立即阻止新操作，并终止或安全关闭该 Token 的活动 Session。
9. 所有权限判断必须在后端完成。
10. 不能通过修改 MCP 参数中的 connection_id 绕过授权。

建议采用 Token 与 Connection 的多对多关系表。

每次 MCP 请求都需要校验：

- Token 是否存在。
- Token 是否启用。
- Token 是否过期。
- 连接是否存在。
- 连接是否启用。
- Token 是否被授权访问该连接。
- 当前操作是否在允许的能力范围内。

## 6.3 Token 安全

- 不允许 MCP 读取完整 Token。
- 不允许 MCP 获取连接密码或私钥。
- 不允许 Token 读取其他 Token 的信息。
- 不允许 Token 管理访问控制设置。
- 不允许 Token 自行给自己增加连接授权。
- 不允许普通 MCP 操作修改审计日志。
- 不允许通过参数注入访问未授权连接。

---

# 七、MCP Server

## 7.1 服务配置

内置 MCP Server。

使用标准 Streamable HTTP Transport。

默认配置：

| 配置 | 默认值 |
|---|---|
| Host | 127.0.0.1 |
| Port | 17321 |
| Endpoint | /mcp |
| Transport | streamable-http |
| Authentication | Bearer Token |

默认 MCP 地址：

http://127.0.0.1:17321/mcp

要求：

1. 默认只监听 127.0.0.1。
2. 不允许未经用户明确配置直接暴露公网。
3. 使用访问控制页面生成的 Token。
4. 每个请求都校验 Token。
5. 每个工具调用都校验连接权限。
6. 支持服务启动和停止。
7. 支持运行状态查询。
8. 支持端口配置。
9. 支持端口占用检测。
10. 服务异常时提供明确错误日志。

MCP Server 必须使用标准协议实现，不要自己实现不完整的 MCP JSON-RPC 子集。

## 7.2 客户端配置示例

首页提供标准配置示例：

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

其中 YOUR_ACCESS_TOKEN 替换为用户在访问控制页面创建的 Token。

注意：

- 不允许硬编码 Token。
- 不允许将真实 Token 放入默认配置。
- 不同客户端的配置文件位置和配置格式可能不同。
- 首页可以提供通用配置示例，并在后续扩展不同客户端的配置指引。

## 7.3 MCP Tools

设计统一的 MCP 工具集合。

### 连接管理

`list_connections`

列出当前 Token 已授权的连接。

仅返回：

- Connection ID。
- 连接名称。
- 协议类型。
- 连接描述。
- 支持的能力。
- 连接状态。

不得返回敏感凭据。

`get_connection_info`

获取当前 Token 有权访问的连接信息。

`test_connection`

测试指定连接。

### Session 管理

`open_session`

创建终端 Session。

参数：

- connection_id。
- terminal_type。
- rows。
- cols。

`execute_command`

执行非交互式命令。

参数：

- connection_id。
- command。
- working_directory。
- timeout。

`send_input`

向已有交互式 Session 发送输入。

参数：

- session_id。
- input。

`read_output`

读取 Session 输出。

参数：

- session_id。
- offset 或其他合理的增量读取参数。

`resize_terminal`

调整终端大小。

参数：

- session_id。
- rows。
- cols。

`close_session`

关闭 Session。

参数：

- session_id。

每个 Session 必须绑定创建它的 Token ID 和 Connection ID。

其他 Token 不能操作不属于自己的 Session。

### 能力约束

SSH：

- Shell。
- Exec。
- InteractiveTerminal。
- TerminalResize。
- 可扩展的设备信息读取。

Telnet：

- Shell。
- Exec。
- InteractiveTerminal。
- TerminalResize（实际支持时）。

WSL：

- Shell。
- Exec。
- InteractiveTerminal。
- TerminalResize。
- 本地 Linux 环境信息。

不能把文件传输工具暴露给当前不支持文件传输的协议。

所有 MCP 工具都必须返回结构化执行结果。

---

# 八、首页快速开始

首页增加「快速开始」功能。

目标是让用户可以快速将 Universal MCP Terminal 接入其他 AI 客户端。

采用简单清晰的步骤引导。

## Step 1：启动 MCP Server

展示：

- MCP 服务运行状态。
- 服务地址。
- 服务端口。
- 启动按钮。
- 停止按钮。

## Step 2：创建访问 Token

如果没有 Token，提示创建。

提供跳转访问控制页面的入口。

## Step 3：授权连接

展示已有连接。

可以快速选择希望交给 AI 使用的连接。

授权开关与访问控制页面实时保持一致。

## Step 4：复制 MCP 配置

展示 MCP JSON 配置。

提供：

- 复制配置。
- 显示 / 隐藏 Token。
- 重新生成配置。

生成的配置使用当前选中的 Token。

## Step 5：复制 AI 接入提示词

提供一个「复制 AI 接入提示词」按钮。

复制以下内容：

---

你现在可以通过 Universal MCP Terminal MCP Server 管理本地授权的远程连接。

MCP 服务地址：

http://127.0.0.1:17321/mcp

Transport：Streamable HTTP。

Authentication：Bearer Token。

请遵守以下规则：

1. 只能使用 Universal MCP Terminal 提供的 MCP Tools 操作设备。
2. 不要自行编写 SSH、Telnet 或 WSL 连接脚本。
3. 不要绕过 Universal MCP Terminal 的访问控制。
4. 只能操作当前 Token 授权的连接。
5. 操作前先调用 list_connections，了解可用连接。
6. 执行命令前说明准备执行的操作。
7. 涉及删除文件、修改配置、重启服务、修改网络等高风险操作，必须先向用户确认。
8. 不要尝试读取、导出或泄露连接密码、私钥及 Token。
9. 所有设备操作必须通过 MCP 完成。
10. 发生连接失败或执行异常时，返回真实错误信息，不要虚构执行结果。

请先调用 list_connections，查看当前 Token 可以访问的连接。

---

提示词中不能包含真实 Token。

---

# 九、Session 管理与审计日志

增加独立的审计日志页面。

目标是记录 AI 通过 MCP 对每个连接执行的操作。

## 9.1 Session 管理

每次创建终端会话时生成唯一 Session ID。

记录：

- Session ID。
- Token ID。
- Token 名称。
- AI 客户端标识（可获取时）。
- Connection ID。
- 连接名称。
- 协议类型。
- 开始时间。
- 结束时间。
- Session 状态。

Session 状态：

- Connecting。
- Connected。
- Running。
- Closed。
- Failed。
- Interrupted。

每个 Session 必须记录所属 Token。

不同 Token 之间不能查看或操作其他 Token 的 Session，除非使用本地管理员界面查看。

## 9.2 命令审计

每次命令执行记录：

- Audit ID。
- Session ID。
- Token ID。
- Connection ID。
- 执行命令。
- 工作目录。
- 执行时间。
- 执行耗时。
- 执行状态。
- Exit Code（适用时）。
- 标准输出。
- 标准错误输出。
- 错误原因。

对于交互式终端：

按时间顺序记录：

- 用户输入。
- 设备输出。
- 命令执行状态。
- Session 状态变化。

对于 Telnet，需要正确处理终端交互与分页输出。

对于 WSL，需要记录：

- 实际执行命令。
- 工作目录。
- 子进程状态。
- 执行结果。

## 9.3 审计日志查询

支持筛选：

- 时间范围。
- Token。
- 连接。
- 协议。
- Session ID。
- 操作类型。
- 执行状态。

支持：

- 查看 Session 详情。
- 查看命令执行时间线。
- 查看完整命令。
- 查看执行输出。
- 查看失败原因。
- 导出审计记录。

## 9.4 审计安全

- 不允许记录完整 Token。
- 不允许记录连接密码。
- 不允许记录私钥。
- 不允许记录认证口令。
- 对敏感命令输出进行脱敏。
- 记录权限拒绝事件。
- 记录连接授权变更。
- 记录 Token 创建、禁用和撤销。
- 普通 MCP 调用方不能修改审计日志。
- 审计记录不能因软件重启而丢失。

必须区分：

- 命令已发送。
- 执行中。
- 执行成功。
- 执行失败。
- 执行超时。
- 执行结果未知。

不能把网络断开简单标记成命令执行失败，因为命令可能已经在远端执行。

---

# 十、安全执行策略

安全控制必须作为核心基础设施实现，而不是后期补充。

## 10.1 执行模式

建议提供三种模式：

### ReadOnly

默认模式。

允许：

- 查看系统信息。
- 查看 CPU、内存、磁盘。
- 查看网络状态。
- 查看进程。
- 查看系统版本。
- 查看目录。
- 读取允许访问的文件。

禁止修改系统状态。

### Restricted

受限执行模式。

允许用户配置额外的命令和操作规则。

例如：

- 指定目录内的文件操作。
- 指定服务的状态查询。
- 指定命令的执行。
- 指定设备的只读管理。

### FullAccess

完整执行模式。

需要用户明确启用。

高风险操作仍然需要按照策略进行确认。

## 10.2 高风险操作

以下操作默认需要用户确认：

- 删除文件或目录。
- 修改系统配置。
- 修改网络配置。
- 重启或关闭设备。
- 创建或删除系统用户。
- 修改防火墙。
- 修改文件权限。
- 执行未知脚本。
- 修改系统服务。
- 安装或卸载软件。

不能只依靠字符串黑名单。

需要结合：

- 命令策略。
- 参数校验。
- 执行模式。
- 目录限制。
- Session 权限。
- 用户确认机制。

对于 SSH、Telnet 等远程 Shell，必须明确说明：通用 Shell 命令无法仅通过字符串过滤获得可靠的只读保证。

对于高风险环境，应优先使用远程受限账号、系统级权限控制、容器隔离或专门的命令代理。

## 10.3 Session 安全

- Session 必须绑定 Token。
- Session 必须绑定连接。
- 不允许跨 Token 操作 Session。
- 连接授权撤销后立即阻止后续操作。
- Token 撤销后立即终止其活动 Session。
- Session 必须具有超时时间。
- 断开连接后正确释放资源。
- 防止长期闲置 Session 占用资源。

---

# 十一、推荐技术栈

项目以 Rust 为核心开发语言。

如果现有项目技术栈已经确定，先评估后再决定是否调整，不要盲目重写。

推荐技术：

| 模块 | 技术 |
|---|---|
| 语言 | Rust |
| 异步运行时 | Tokio |
| SSH | russh |
| Telnet | Telnet 相关成熟 Crate，或基于 Tokio 实现 |
| WSL | std::process::Command |
| MCP | rmcp |
| 序列化 | serde |
| 日志 | tracing |
| 配置管理 | config |
| 数据库 | SQLite |
| 数据访问 | sqlx |
| 桌面 UI | Tauri 2 |
| 前端 | 根据现有项目技术栈复用 |

依赖选择必须检查当前版本兼容性、维护状态和许可证。

不要使用过时 API。

不要为了统一而强行使用不适合的第三方库。

---

# 十二、推荐项目目录

采用 Rust Workspace + 多 Crate 架构。

参考结构：

```text
universal_mcp_terminal/
├── Cargo.toml
├── crates/
│   ├── core/
│   ├── protocol-api/
│   ├── protocol-registry/
│   ├── providers/
│   │   ├── ssh/
│   │   ├── telnet/
│   │   └── wsl/
│   ├── session-manager/
│   ├── mcp-server/
│   ├── access-control/
│   ├── audit/
│   ├── persistence/
│   ├── security/
│   └── app/
├── frontend/
├── docs/
│   ├── architecture/
│   ├── protocols/
│   ├── security/
│   ├── mcp/
│   └── development/
└── README.md
```

目录仅为建议。

实际实现时应结合现有代码决定，不必为了形式拆分过多 Crate。

要求：

- Provider 可以独立开发。
- 核心业务不依赖具体协议。
- MCP 层不依赖具体协议。
- 审计层不依赖具体协议。
- 权限层不依赖具体协议。
- UI 不硬编码协议字段。

---

# 十三、UI 页面规划

导航建议：

1. 首页。
2. 连接管理。
3. 访问控制。
4. 审计日志。
5. 系统设置。

## 首页

展示：

- MCP Server 运行状态。
- 已创建连接数量。
- 已创建 Token 数量。
- 活跃 Session 数量。
- 最近操作记录。
- 快速开始。

## 连接管理

支持：

- 连接列表。
- 协议筛选。
- 连接创建。
- 连接编辑。
- 删除连接。
- 测试连接。
- 连接状态。
- 授权状态。

连接创建表单必须根据 Provider Schema 动态生成。

## 访问控制

展示：

- Token 列表。
- Token 名称。
- 脱敏 Token。
- 创建时间。
- 有效期。
- 最后使用时间。
- 启用状态。

Token 详情页面展示连接授权开关。

## 审计日志

展示：

- Session 列表。
- 命令执行记录。
- 操作时间线。
- 执行状态。
- 详细输出。
- 操作筛选。
- 导出功能。

## 系统设置

展示：

- MCP Server 配置。
- 服务运行状态。
- 监听地址。
- 端口。
- 日志设置。
- 默认安全执行模式。
- Session 超时设置。

---

# 十四、实施阶段

请先分析项目，再分阶段开发。

不要一次性盲目修改所有代码。

## 第一阶段：项目分析与数据重构

- 阅读项目现有代码。
- 了解技术栈。
- 检查数据恢复问题。
- 检查数据模型。
- 检查持久化机制。
- 制定数据迁移方案。
- 必要时重写数据模块。

交付：

- 项目现状分析。
- 问题清单。
- 重构方案。
- 数据迁移方案。

## 第二阶段：核心架构

- 建立 Protocol API。
- 建立 Provider Registry。
- 建立 Capability 模型。
- 建立 Session Manager。
- 建立统一错误处理。
- 建立持久化模块。

完成 SSH、Telnet、WSL Provider 的注册和接口测试。

## 第三阶段：连接管理

- 创建连接。
- 编辑连接。
- 删除连接。
- 测试连接。
- 连接状态管理。
- 敏感信息加密存储。
- 应用重启后的数据恢复。

## 第四阶段：访问控制

- Token 创建。
- Token 启用和禁用。
- Token 撤销。
- Token 有效期。
- Token 与连接多对多授权。
- 连接授权开关。
- 后端实时权限校验。

## 第五阶段：MCP Server

- Streamable HTTP。
- Bearer Token。
- MCP 工具注册。
- 连接列表。
- 连接测试。
- Session 管理。
- 命令执行。
- 交互式终端。
- 连接权限校验。

## 第六阶段：首页快速开始

- MCP 服务状态。
- Token 创建入口。
- 连接授权展示。
- MCP JSON 配置。
- AI 接入提示词。
- 一键复制。

## 第七阶段：审计日志

- Session 审计。
- 命令审计。
- 输入输出记录。
- 错误记录。
- 权限拒绝记录。
- Token 操作记录。
- 查询和导出。

## 第八阶段：安全测试与整体验收

完成协议测试、权限测试、持久化测试和 MCP 客户端接入测试。

每个阶段都需要：

1. 编译。
2. 类型检查。
3. 单元测试。
4. 相关集成测试。
5. 修复问题。
6. 更新文档。

---

# 十五、验收标准

必须完成以下测试：

### 数据持久化

1. 软件重启后连接仍然存在。
2. Token 仍然存在。
3. 连接授权关系仍然存在。
4. 审计记录仍然存在。
5. 数据升级不会导致无故丢失。

### 协议

1. SSH 可以真实认证并执行命令。
2. Telnet 可以完成登录交互并执行命令。
3. WSL 可以调用指定发行版。
4. 三种协议都能创建和关闭 Session。
5. 连接失败能够返回真实错误。
6. 不支持的能力不会被伪装成成功。

### Token

1. 可以创建多个 Token。
2. 一个 Token 可以授权多个连接。
3. 一个连接可以授权多个 Token。
4. 关闭授权后立即失效。
5. 撤销 Token 后立即失效。
6. 不同 Token 不能互相操作 Session。
7. 无法通过修改 connection_id 绕过权限。

### MCP

1. 标准 MCP 客户端能够连接。
2. Bearer Token 能够正确认证。
3. 未认证请求被拒绝。
4. 过期 Token 被拒绝。
5. 已撤销 Token 被拒绝。
6. 未授权连接无法访问。
7. MCP 默认仅监听本机地址。

### 审计

1. 每个 Session 有唯一 ID。
2. 每条命令有完整记录。
3. 可以查询输入输出时间线。
4. 可以查看失败原因。
5. 可以区分超时和执行失败。
6. 敏感信息得到脱敏。
7. 审计记录持久化。

### 安全

1. 连接密码不能明文保存。
2. Token 不能明文保存。
3. 私钥不能进入普通日志。
4. 未授权操作被后端拒绝。
5. Token 撤销后活动 Session 被终止。
6. 高风险操作按照策略要求用户确认。

---

# 十六、开发约束

1. 先阅读现有项目，不要假设现有技术架构。
2. 优先复用已有可用模块。
3. 协议必须通过 Provider 扩展。
4. 不要把协议专属逻辑写入核心业务。
5. 不要在 UI 中硬编码协议表单。
6. 不允许只实现前端假数据。
7. 不允许模拟连接成功。
8. 不允许只在前端实现权限控制。
9. 不允许硬编码 Token 和连接密码。
10. 不允许忽略数据持久化。
11. 不要实现 FTP、FTPS、SFTP。
12. 不要擅自删除与本次任务无关的功能。
13. 不要擅自修改密钥、环境变量和敏感配置。
14. 不要擅自 Git Commit 或 Push。
15. 涉及用户数据删除、不可逆迁移时必须先征求确认。

如果遇到需求不明确的地方，先分析并提出合理方案，不要自行做可能破坏数据或安全边界的决定。

---

# 十七、最终交付

最终交付必须包括：

1. 可正常运行的 Universal MCP Terminal。
2. 可扩展的协议 Provider 架构。
3. SSH、Telnet、WSL 三种真实连接能力。
4. 可靠的数据持久化。
5. Token 管理。
6. Token 与连接独立授权。
7. 本地 MCP Server。
8. 首页快速开始。
9. AI 接入提示词复制。
10. Session 管理。
11. 完整操作审计。
12. 命令安全控制。
13. 项目架构文档。
14. MCP 接入文档。
15. 协议开发文档。
16. 测试报告。

**最终目标：Universal MCP Terminal 不是一个简单的 SSH 终端，而是一个可扩展、可授权、可审计、可被各种本地 AI Agent 安全调用的通用终端连接平台。**

最重要的三个原则：

- 协议可以无限扩展，核心架构不随协议增加而重写。
- AI 只能访问 Token 明确授权的连接。
- 所有 AI 操作都必须经过安全控制，并留下可追溯的审计记录。