use crate::error::SessionError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use umt_core::{Capability, CapabilitySet, ExecutionState, SessionId};

/// 敏感输入值（密码、私钥、口令）。
///
/// Debug 输出永远不包含真实内容，避免被 tracing 意外记录。
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretValues(std::collections::BTreeMap<String, String>);

impl SecretValues {
    pub fn new(values: std::collections::BTreeMap<String, String>) -> Self {
        Self(values)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SecretValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretValues(<redacted:{}>)", self.0.len())
    }
}

/// 建立会话时的通用参数。与协议无关，具体协议自行决定忽略哪些字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectOptions {
    pub rows: u16,
    pub cols: u16,
    pub terminal_type: Option<String>,
    pub working_dir: Option<String>,
    pub env: Vec<(String, String)>,
    pub timeout_ms: u64,
    /// 敏感配置（密码、私钥等），来自加密存储，由 App 层解密后传入。
    #[serde(default)]
    pub secrets: SecretValues,
    /// 已固定的 Host Key 指纹；`None` 表示尚未信任，SSH Provider 必须返回 `HostKeyUnknown`。
    #[serde(default)]
    pub expected_host_key_fingerprint: Option<String>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            terminal_type: None,
            working_dir: None,
            env: Vec::new(),
            timeout_ms: 15_000,
            secrets: SecretValues::default(),
            expected_host_key_fingerprint: None,
        }
    }
}

/// 增量输出。`cursor` 是下一次读取的起点，由 Provider 维护单调递增。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputChunk {
    pub cursor: u64,
    /// 已经过 UTF-8 lossy 转换的终端输出。
    pub text: String,
    pub eof: bool,
    /// 因缓冲区上限导致中间有丢失时为 true，调用方需要知道输出不完整。
    pub truncated: bool,
}

/// 非交互命令执行请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub working_dir: Option<String>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

impl ExecRequest {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_dir: None,
            timeout_ms: 30_000,
            max_output_bytes: 256 * 1024,
        }
    }
}

/// 非交互命令执行结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    pub state: ExecutionState,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

impl ExecResult {
    pub fn succeeded(stdout: String, exit_code: Option<i32>, duration_ms: u64) -> Self {
        Self {
            state: ExecutionState::Succeeded,
            exit_code,
            stdout,
            stderr: String::new(),
            duration_ms,
            truncated: false,
        }
    }
}

/// 一个已建立的终端会话。
///
/// 生命周期由 Session Manager 持有；Provider 必须保证 `close` 之后释放所有资源
/// （SSH channel、TCP socket、WSL 子进程）。
#[async_trait]
pub trait TerminalSession: Send {
    fn session_id(&self) -> SessionId;

    fn capabilities(&self) -> &CapabilitySet;

    /// 写入原始输入（交互式终端）。
    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError>;

    /// 读取 `cursor` 之后的新输出；没有新数据时返回空 `text`，由调用方决定是否等待。
    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError>;

    /// 调整终端尺寸。不支持 `TerminalResize` 时必须返回 `UnsupportedCapability`。
    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError>;

    /// 执行非交互命令。不支持 `Exec` 的协议默认返回 `UnsupportedCapability`。
    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        let _ = request;
        Err(SessionError::UnsupportedCapability(Capability::Exec))
    }

    /// 关闭会话并释放资源。
    async fn close(self: Box<Self>) -> Result<(), SessionError>;
}
