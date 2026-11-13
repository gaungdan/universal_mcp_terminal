# 协议 Provider 开发指南

> 目标：让新增协议只增加 Provider，不修改 Token、会话、MCP、审计、权限与 UI。
> 契约代码：`crates/protocol-api/src/{provider,session,schema,test_result,error}.rs`。
> 现有实现：`crates/providers/{ssh,telnet,wsl}`。

## 一、`ProtocolProvider` 接口

```rust
#[async_trait]
pub trait ProtocolProvider: Send + Sync {
    fn descriptor(&self) -> &ProtocolDescriptor;
    fn config_schema(&self) -> &ConfigSchema;
    async fn validate_config(&self, config: &ConnectionConfig) -> Result<(), ProviderError>;
    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError>;
    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError>;
}
```

| 方法 | 是否必须实现 | 语义与约束 |
|---|---|---|
| `descriptor` | 是 | 返回 `&ProtocolDescriptor`，包含 `protocol_id`、`display_name`、`version`、`description`、`capabilities` |
| `config_schema` | 是 | 返回稳定的 `&ConfigSchema`；字段 `key` 发布后不得重命名 |
| `validate_config` | 否（有默认实现） | 默认校验 `protocol_id` 一致、`values` 是 JSON 对象、按 Schema 校验字段；可覆盖以增加语义校验（例如 Telnet 的正则合法性、WSL 的发行版放宽） |
| `test_connection` | 是 | 必须完成真实握手/认证，禁止只探测端口；失败应返回 `ConnectionTestResult { ok: false, steps, suggestion }` 或分类后的 `ProviderError` |
| `connect` | 是 | 建立真实连接并返回 `Box<dyn TerminalSession>`；敏感值只能从 `ConnectOptions.secrets` 读取 |

`ProtocolDescriptor` 的 `protocol_id` 必须全局唯一，一旦发布不可更改（既有连接、审计和配置都按它索引）。

`ConnectOptions` 携带：

| 字段 | 说明 |
|---|---|
| `rows` / `cols` | 初始终端尺寸 |
| `terminal_type` | 终端类型，可覆盖连接配置 |
| `working_dir` | 工作目录，可覆盖连接配置 |
| `env` | 额外环境变量 |
| `timeout_ms` | 建立连接的通用超时 |
| `secrets` | 解密后的敏感字段（`SecretValues`，`Debug` 永远输出 `<redacted:n>`） |
| `expected_host_key_fingerprint` | 已信任的 Host Key 指纹；`None` 时 SSH 必须返回 `HostKeyUnknown` |

## 二、`TerminalSession` 接口

```rust
#[async_trait]
pub trait TerminalSession: Send {
    fn session_id(&self) -> SessionId;
    fn capabilities(&self) -> &CapabilitySet;
    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError>;
    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError>;
    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError>;
    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        Err(SessionError::UnsupportedCapability(Capability::Exec))
    }
    async fn close(self: Box<Self>) -> Result<(), SessionError>;
}
```

| 方法 | 语义与约束 |
|---|---|
| `session_id` | 会话唯一 ID，由 Provider 创建时生成并保持不变 |
| `capabilities` | 通常直接返回描述符中的能力集合 |
| `write` | 写入原始字节（交互式输入）；会话已关闭返回 `SessionError::Closed` |
| `read(cursor, max_bytes)` | 增量读取 `cursor` 之后的新输出；返回新的 `cursor`（单调递增）、`text`、`eof`、`truncated`；没有新数据时返回空 `text` |
| `resize` | 调整终端尺寸；不支持 `TerminalResize` 时必须返回 `UnsupportedCapability`，不能静默成功 |
| `exec` | 非交互命令；默认返回 `UnsupportedCapability(Exec)`；结果必须区分 `Succeeded` / `Failed` / `TimedOut` / `Unknown` |
| `close` | 释放所有资源（SSH channel、TCP socket、WSL 子进程）；`SessionManager` 关闭后会把占位会话替换为 `ClosedSession` |

`OutputChunk`：

```rust
pub struct OutputChunk {
    pub cursor: u64,
    pub text: String,   // 已做 UTF-8 lossy 转换
    pub eof: bool,
    pub truncated: bool, // 缓冲上限导致中间输出丢失
}
```

`ExecResult` 必须包含 `state`、`exit_code`、`stdout`、`stderr`、`duration_ms`、`truncated`；
`ExecRequest` 包含 `command`、`working_dir`、`timeout_ms`、`max_output_bytes`。

## 三、`ConfigSchema` 与前端 `SchemaForm`

`ConfigSchema` 是协议自描述表单的唯一来源；前端 `apps/desktop/src/components/SchemaForm.tsx`
按下述结构渲染，不允许在页面中写 `if (protocol === "ssh")`。

```rust
pub struct ConfigSchema {
    pub protocol_id: String,
    pub fields: Vec<ConfigField>,
}

pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
    pub placeholder: Option<String>,
    pub help: Option<String>,
    pub secret: bool,
    pub visible_when: Option<FieldCondition>,
    pub validation: Option<FieldValidation>,
}
```

### 3.1 字段类型 `FieldKind`

| `kind` | 前端控件 | 值类型 |
|---|---|---|
| `text` | 单行文本框 | string |
| `password` | 密码框（带显示/隐藏按钮） | string |
| `number` | 数字输入（`step="any"`） | number（实际提交由后端按 i64 校验） |
| `boolean` | 复选框 | bool |
| `select` | 下拉框，`options: [{ value, label }]` | string，必须命中某个 `value` |
| `multiline` | 多行文本框（4 行） | string |
| `path` | 普通单行文本框（路径提示由 `placeholder/help` 提供） | string |

### 3.2 `secret` 语义

- `secret = true` 的字段：
  - 前端把值写入独立的 `secrets` 映射，**不进入** `config.values`；
  - 后端 `split_config` 在保存前从配置对象中移除该字段，写入 `connection_secrets`（AES-256-GCM）；
  - 读取连接详情时只返回 `secret_fields_set` 字段名，不回显值；
  - MCP 执行时由 `load_secrets` 解密后放入 `ConnectOptions.secrets`；
  - `test_connection` 没有 `ConnectOptions`，由调用方把敏感值临时合入配置（仅内存）。
- 非 `secret` 字段保存到 `connections.config_json`，会随连接详情回显。
- 敏感字段不应设置 `required = true` 来强制“已有密文”；是否必填由 Provider 在 `connect`/`validate_config` 中结合 `secrets` 判断（参考 SSH 密码/私钥二选一）。

### 3.3 `visible_when` 语义

```rust
pub struct FieldCondition {
    pub key: String,
    pub equals: serde_json::Value,
}
```

- 仅当另一个字段的当前值“宽松等于”`equals`（字符串化后比较）时显示。
- 前端在条件不满足时会**删除**该字段在 `values` 与 `secrets` 中的值，避免把不可见字段提交给后端。
- 典型用法：SSH 的 `password` 仅在 `auth_type = "password"` 时显示，`private_key` 与 `private_key_passphrase` 仅在 `auth_type = "private_key"` 时显示。

### 3.4 `validation`

```rust
pub struct FieldValidation {
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub pattern: Option<String>,
}
```

- 字符串类字段使用 `min_length` / `max_length` / `pattern`；数字类字段使用 `min` / `max`。
- `pattern` 必须同时能被 Rust `regex` 与浏览器 `RegExp` 接受（避免回溯类语法）。
- 前端只做即时提示；后端 `ConfigSchema::validate` 是最终裁决，且会拒绝 Schema 未声明的字段（防协议注入）。

### 3.5 默认值

- 前端在字段可见且当前值为空时填入 `default_value`；`secret` 字段不自动填默认值。
- 后端不信任前端默认值，仍按 Schema 校验；缺省值语义由 Provider 的 `from_config` 决定（例如 SSH `port` 默认 22、Telnet `port` 默认 23、WSL `shell` 默认 `/bin/bash`）。

## 四、Capability 声明规则

1. 只声明**真实支持**的能力，不能为了“看起来完整”多声明。
2. 支持 `exec` 才声明 `Capability::Exec`；否则 MCP `execute_command` 必须失败。
3. 支持尺寸调整才声明 `TerminalResize`；Telnet 当前**不声明**，`resize_terminal` 返回 `unsupported_capability`。
4. 交互式终端声明 `Shell` + `InteractiveTerminal`。
5. 能力集合会被序列化为字符串数组，稳定标识见 `Capability::as_str()`：
   `shell`、`exec`、`interactive_terminal`、`device_information`、`port_forward`、`terminal_resize`、`environment_info`。
6. 不支持的能力必须明确报错：
   - MCP 层：`ensure_capability` → `UmtError::UnsupportedCapability`（`-32003`）；
   - 会话层：`SessionError::UnsupportedCapability`；
   - 禁止模拟成功或返回空结果冒充成功。

## 五、最小实现骨架（假想协议 `serial`）

以下代码仅作结构示例，不落盘、不参与编译。真实实现需替换 `SerialSession` 的读写逻辑。

```rust
// crates/providers/serial/src/lib.rs
pub mod config;
pub mod provider;
pub mod session;

pub use provider::SerialProvider;
```

```rust
// crates/providers/serial/src/provider.rs
use async_trait::async_trait;
use umt_core::{Capability, CapabilitySet, ConnectionConfig};
use umt_protocol_api::{
    ConfigField, ConfigSchema, ConnectOptions, ConnectionTestResult, FieldKind, FieldValidation,
    ProtocolDescriptor, ProtocolProvider, ProviderError, TerminalSession, TestStep,
};

pub struct SerialProvider {
    descriptor: ProtocolDescriptor,
    schema: ConfigSchema,
}

impl SerialProvider {
    pub fn new() -> Self {
        Self {
            descriptor: ProtocolDescriptor {
                protocol_id: "serial".into(),
                display_name: "Serial".into(),
                version: "1".into(),
                description: "串口终端（RS232/RS485）".into(),
                capabilities: CapabilitySet::of([
                    Capability::Shell,
                    Capability::InteractiveTerminal,
                ]),
            },
            schema: ConfigSchema::new(
                "serial",
                vec![
                    ConfigField {
                        key: "port".into(),
                        label: "串口".into(),
                        kind: FieldKind::Text,
                        required: true,
                        default_value: None,
                        placeholder: Some("COM3".into()),
                        help: Some("Windows 下形如 COM3".into()),
                        secret: false,
                        visible_when: None,
                        validation: Some(FieldValidation {
                            min_length: Some(1),
                            ..Default::default()
                        }),
                    },
                    ConfigField {
                        key: "baud_rate".into(),
                        label: "波特率".into(),
                        kind: FieldKind::Number,
                        required: true,
                        default_value: Some(115_200.into()),
                        placeholder: None,
                        help: None,
                        secret: false,
                        visible_when: None,
                        validation: Some(FieldValidation {
                            min: Some(1),
                            max: Some(4_000_000),
                            ..Default::default()
                        }),
                    },
                ],
            ),
        }
    }
}

#[async_trait]
impl ProtocolProvider for SerialProvider {
    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> &ConfigSchema {
        &self.schema
    }

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError> {
        self.validate_config(config).await?;
        // 必须真实打开串口并完成一次可验证的读写，不能只检查设备名存在。
        let port = config
            .get_str("port")
            .ok_or_else(|| ProviderError::ConfigInvalid {
                field: "port".into(),
                message: "缺少串口名称".into(),
            })?;

        Ok(ConnectionTestResult::success(
            format!("串口 {port} 打开成功"),
            1,
            vec![TestStep::passed("open", Some(1))],
        ))
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError> {
        self.validate_config(config).await?;
        let session = crate::session::SerialSession::open(config, options).await?;
        Ok(Box::new(session))
    }
}
```

```rust
// crates/providers/serial/src/session.rs（骨架）
use async_trait::async_trait;
use umt_core::{CapabilitySet, SessionId};
use umt_protocol_api::{
    ConnectOptions, ExecRequest, ExecResult, OutputChunk, SessionError, TerminalSession,
};

pub struct SerialSession {
    id: SessionId,
    capabilities: CapabilitySet,
    // reader / writer / buffer / closed ...
}

#[async_trait]
impl TerminalSession for SerialSession {
    fn session_id(&self) -> SessionId {
        self.id
    }

    fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError> {
        // 写入串口；已关闭返回 SessionError::Closed
        let _ = data;
        Ok(())
    }

    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError> {
        // 从环形缓冲读取 cursor 之后的数据
        let _ = max_bytes;
        Ok(OutputChunk {
            cursor,
            text: String::new(),
            eof: false,
            truncated: false,
        })
    }

    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError> {
        // 串口不支持 TerminalResize（描述符中也未声明）
        let _ = (rows, cols);
        Err(SessionError::UnsupportedCapability(
            umt_core::Capability::TerminalResize,
        ))
    }

    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        // 串口没有“退出码”概念，应返回明确语义，而不是伪造 Success
        let _ = request;
        Err(SessionError::UnsupportedCapability(umt_core::Capability::Exec))
    }

    async fn close(self: Box<Self>) -> Result<(), SessionError> {
        // 关闭串口句柄、释放读写任务
        Ok(())
    }
}
```

注册（唯一需要触碰的现有文件）：

```rust
// crates/app/src/services/state.rs
registry.register(Arc::new(SshProvider::new()))?;
registry.register(Arc::new(TelnetProvider::new()))?;
registry.register(Arc::new(WslProvider::new()))?;
registry.register(Arc::new(SerialProvider::new()))?; // 新增
```

## 六、测试要求

Provider 交付前至少覆盖：

1. **配置校验**
   - 缺少必填字段、字段类型错误、未知字段、越界数值、非法正则都必须被拒绝；
   - `secret` 字段与 `visible_when` 条件与描述符一致；
   - `protocol_id` 与 Provider 不一致时返回 `ConfigInvalid`。
2. **错误分类**
   - 认证失败 → `AuthFailed`（映射 `access_denied`）；
   - 超时 → `Timeout`（映射 `timeout`）；
   - 网络/断开/I/O → `Network` / `Disconnected` / `Io`（映射 `external`）；
   - 未知 Host Key / 指纹不匹配 → `HostKeyUnknown` / `HostKeyMismatch`（映射 `access_denied`）；
   - 不支持能力 → `UnsupportedCapability`（映射 `unsupported_capability`）。
3. **真实/模拟连接**
   - 无外部依赖的自动化测试使用模拟服务端（参考 Telnet 的 `spawn_device`）或本机进程（参考 WSL）；
   - 依赖真实服务器的测试必须 `#[ignore]` 并用环境变量门控，例如
     `UMT_TEST_SSH_HOST/PORT/USER/PASSWORD`，通过 `cargo test -p umt-provider-ssh -- --ignored` 运行；
   - 测试必须校验真实的 `stdout`、`exit_code`、交互回显，不能只断言“没有报错”。
4. **生命周期**
   - `write` → `read(cursor)` 的游标推进正确；
   - `close` 后 `write/read/resize` 返回 `Closed`；
   - `close`/`Drop` 必须释放子进程、socket 或句柄，测试中应有“无残留进程/连接”断言。
5. **能力约束**
   - 描述符声明与 `TerminalSession` 实际行为一致；
   - 未声明能力必须返回 `UnsupportedCapability`，并有对应单测。
