//! WSL 会话：交互式终端（stdin/stdout 管道）+ 一次性命令执行 + 进程树清理。

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use umt_core::{Capability, CapabilitySet, ExecutionState, SessionId};
use umt_protocol_api::error::{ProviderError, SessionError};
use umt_protocol_api::session::{
    ExecRequest, ExecResult, FileChunk, FileReadRequest, OutputChunk, TerminalSession,
};

use crate::config::WslConfig;
use crate::decode;
use crate::job::JobGuard;
use crate::process::{self, WslCommand};

/// 交互式输出缓冲区上限，超出后丢弃最旧的数据并把 `truncated` 标记透出。
const MAX_BUFFER_BYTES: usize = 256 * 1024;

/// 一个正在运行的 WSL 交互终端会话。
pub struct WslSession {
    id: SessionId,
    capabilities: CapabilitySet,
    settings: WslConfig,
    /// 子进程；`close` / `Drop` 后置为 None。
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    /// stdout/stderr 的增量输出，由两个后台读取任务持续写入。
    output: Arc<Mutex<SessionOutput>>,
    readers: Vec<JoinHandle<()>>,
    /// 进程树清理依赖的 Job Object；为 None 表示创建失败（降级为只杀直接子进程）。
    job: Option<JobGuard>,
    rows: u16,
    cols: u16,
    closed: bool,
}

impl WslSession {
    /// 启动 `wsl.exe -d <distro> [--cd <dir>] -- <shell>` 交互式会话。
    pub(crate) fn spawn(
        capabilities: CapabilitySet,
        settings: WslConfig,
        rows: u16,
        cols: u16,
    ) -> Result<Self, ProviderError> {
        let mut command = process::build_command(&settings.env);
        command
            .args(settings.interactive_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| {
            ProviderError::Io(format!(
                "无法启动 wsl.exe（发行版 {}）：{error}",
                settings.distribution
            ))
        })?;

        // 先入 Job 再取走管道：之后无论 close 还是 Drop，句柄关闭都会结束整棵进程树。
        let job = JobGuard::create();
        if let Some(job) = &job
            && let Some(process) = process::child_raw_handle(&child)
            && let Err(error) = job.assign(process)
        {
            tracing::warn!(%error, "WSL 会话子进程加入 Job Object 失败，Drop 时只能结束直接子进程");
        }

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let output = Arc::new(Mutex::new(SessionOutput::new(2)));
        let readers = vec![
            spawn_reader(stdout, output.clone()),
            spawn_reader(stderr, output.clone()),
        ];

        Ok(Self {
            id: SessionId::new(),
            capabilities,
            settings,
            child: Some(child),
            stdin,
            output,
            readers,
            job,
            rows,
            cols,
            closed: false,
        })
    }

    /// 终止子进程并释放全部资源。
    async fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;

        // 关闭 stdin，让 shell 有机会自行退出。
        self.stdin.take();
        if let Some(job) = &self.job {
            job.terminate();
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        }
        for reader in self.readers.drain(..) {
            reader.abort();
        }
        // 关闭 Job 句柄：KILL_ON_JOB_CLOSE 会清掉仍挂在 Job 上的残留进程。
        self.job = None;
        self.child = None;
    }

    /// 在发行版内执行一条短脚本并捕获原始输出。
    ///
    /// 不走 `startup_args`（默认 `-lc`）：读取文件不需要登录 Shell，
    /// 避免用户的 Shell 配置向 stdout 写入额外内容污染字节流。
    async fn run_script(
        &self,
        script: &str,
        max_output_bytes: usize,
    ) -> Result<process::CommandOutcome, SessionError> {
        let mut args = self.settings.base_args(None);
        args.push("-c".to_string());
        args.push(script.to_string());

        let outcome = process::run_capture(WslCommand {
            args,
            env: self.settings.env.clone(),
            timeout: self.settings.timeout,
            max_output_bytes,
        })
        .await
        .map_err(|error| SessionError::Io(format!("无法执行 wsl.exe：{error}")))?;

        if outcome.timed_out {
            return Err(SessionError::Timeout(format!(
                "读取 WSL 文件超时（{} 秒）",
                self.settings.timeout.as_secs()
            )));
        }
        Ok(outcome)
    }

    /// 读取普通文件大小；路径不存在或不可读时返回带 stderr 的错误。
    async fn remote_file_size(&self, path: &str) -> Result<u64, SessionError> {
        let outcome = self
            .run_script(&format!("wc -c < {}", shell_quote(path)), 64 * 1024)
            .await?;
        if outcome.exit_code != Some(0) {
            let stderr = decode::decode_output(&outcome.stderr);
            let stdout = decode::decode_output(&outcome.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            return Err(SessionError::Io(format!("打开 WSL 文件失败：{detail}")));
        }
        decode::decode_output(&outcome.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|_| SessionError::Internal("无法解析 WSL 文件大小".into()))
    }

    /// 测试用：子进程 PID，用于验证关闭后没有进程泄漏。
    #[cfg(test)]
    pub(crate) fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }
}

impl Drop for WslSession {
    fn drop(&mut self) {
        // Drop 不能 await，这里做同步兜底：请求子进程退出后立即关闭 Job 句柄，
        // KILL_ON_JOB_CLOSE 会强制结束整棵进程树，避免 wsl.exe 泄漏。
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        self.job.take();
        for reader in self.readers.drain(..) {
            reader.abort();
        }
    }
}

#[async_trait]
impl TerminalSession for WslSession {
    fn session_id(&self) -> SessionId {
        self.id
    }

    fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(SessionError::Closed);
        };
        stdin
            .write_all(data)
            .await
            .map_err(|error| SessionError::Io(format!("写入 WSL 会话失败：{error}")))?;
        stdin
            .flush()
            .await
            .map_err(|error| SessionError::Io(format!("刷新 WSL 会话输入失败：{error}")))
    }

    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        let output = self.output.lock().await;
        Ok(output.read(cursor, max_bytes))
    }

    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        // WSL 通过管道直连进程，没有可调整的 PTY；记录尺寸供上层查询即可。
        self.rows = rows;
        self.cols = cols;
        Ok(())
    }

    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        if !self.capabilities.contains(Capability::Exec) {
            return Err(SessionError::UnsupportedCapability(Capability::Exec));
        }

        let working_dir = request
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| self.settings.working_dir.clone());
        let timeout = if request.timeout_ms == 0 {
            self.settings.timeout
        } else {
            Duration::from_millis(request.timeout_ms)
        };

        let outcome = process::run_capture(WslCommand {
            args: self
                .settings
                .exec_args(&request.command, working_dir.as_deref()),
            env: self.settings.env.clone(),
            timeout,
            max_output_bytes: request.max_output_bytes,
        })
        .await
        .map_err(|error| SessionError::Io(format!("无法执行 wsl.exe：{error}")))?;

        let state = if outcome.timed_out {
            ExecutionState::TimedOut
        } else {
            match outcome.exit_code {
                Some(0) => ExecutionState::Succeeded,
                Some(_) => ExecutionState::Failed,
                // 子进程被外部结束且没有退出码时，无法确认命令是否执行完成。
                None => ExecutionState::Unknown,
            }
        };

        Ok(ExecResult {
            state,
            exit_code: outcome.exit_code,
            stdout: decode::decode_output(&outcome.stdout),
            stderr: decode::decode_output(&outcome.stderr),
            duration_ms: outcome.duration.as_millis() as u64,
            truncated: outcome.truncated,
        })
    }

    async fn read_file(&mut self, request: FileReadRequest) -> Result<FileChunk, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        if !self.capabilities.contains(Capability::FileRead) {
            return Err(SessionError::UnsupportedCapability(Capability::FileRead));
        }

        let max_bytes = request.max_bytes.max(1);
        let size = self.remote_file_size(&request.path).await?;

        if request.offset > size {
            return Ok(FileChunk {
                path: request.path,
                offset: request.offset,
                next_offset: request.offset,
                size: Some(size),
                eof: true,
                data: Vec::new(),
            });
        }

        // tail 的 +N 从 1 开始计数；head 限制单次读取量，避免大文件整体进入内存。
        let script = format!(
            "tail -c +{} -- {} | head -c {}",
            request.offset + 1,
            shell_quote(&request.path),
            max_bytes
        );
        let outcome = self.run_script(&script, max_bytes).await?;
        if outcome.exit_code != Some(0) {
            let detail = decode::decode_output(&outcome.stderr).trim().to_string();
            return Err(SessionError::Io(format!("读取 WSL 文件失败：{detail}")));
        }

        let data = outcome.stdout;
        let next_offset = request.offset + data.len() as u64;
        let eof = next_offset >= size || data.len() < max_bytes;

        Ok(FileChunk {
            path: request.path,
            offset: request.offset,
            next_offset,
            size: Some(size),
            eof,
            data,
        })
    }

    async fn close(mut self: Box<Self>) -> Result<(), SessionError> {
        self.shutdown().await;
        Ok(())
    }
}

/// 会话增量输出缓冲区，`cursor` 是缓冲区内的绝对字节偏移。
struct OutputBuffer {
    /// 缓冲区首字节对应的绝对偏移；丢弃旧数据时同步前移。
    base: u64,
    data: Vec<u8>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            base: 0,
            data: Vec::new(),
        }
    }

    fn total_len(&self) -> u64 {
        self.base + self.data.len() as u64
    }

    fn push(&mut self, chunk: &[u8]) {
        self.data.extend_from_slice(chunk);
        if self.data.len() > MAX_BUFFER_BYTES {
            let overflow = self.data.len() - MAX_BUFFER_BYTES;
            self.data.drain(..overflow);
            self.base += overflow as u64;
        }
    }
}

/// stdout/stderr 合并后的会话输出状态。
struct SessionOutput {
    buffer: OutputBuffer,
    /// 仍在运行的读取任务数；归零表示两条管道都已关闭（进程退出）。
    pending_readers: usize,
    eof: bool,
}

impl SessionOutput {
    fn new(pending_readers: usize) -> Self {
        Self {
            buffer: OutputBuffer::new(),
            pending_readers,
            eof: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.buffer.push(chunk);
    }

    fn finish_reader(&mut self) {
        self.pending_readers = self.pending_readers.saturating_sub(1);
        if self.pending_readers == 0 {
            self.eof = true;
        }
    }

    /// 取出 `cursor` 之后的增量文本，最多 `max_bytes` 字节。
    ///
    /// 返回的 `cursor` 永远停在 UTF-8 字符边界上，多字节字符被截断时留到下次读取。
    fn read(&self, cursor: u64, max_bytes: usize) -> OutputChunk {
        let total = self.buffer.total_len();
        let mut truncated = false;
        let start = if cursor < self.buffer.base {
            // 旧数据已被丢弃，只能从当前缓冲区起点继续。
            truncated = true;
            self.buffer.base
        } else {
            cursor.min(total)
        };

        let offset = (start - self.buffer.base) as usize;
        let available = &self.buffer.data[offset..];
        if max_bytes == 0 || available.is_empty() {
            return OutputChunk {
                cursor: start,
                text: String::new(),
                eof: self.eof,
                truncated,
            };
        }

        let limit = max_bytes.min(available.len());
        let mut end = limit;
        let text = loop {
            match std::str::from_utf8(&available[..end]) {
                Ok(text) => break text.to_string(),
                // 真正非法的字节：用替换字符兜底，游标继续前进。
                Err(error) if error.error_len().is_some() => {
                    break String::from_utf8_lossy(&available[..end]).into_owned();
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        end = valid;
                    } else if end < available.len() {
                        // 末尾字符不完整，向后多取一个字节再试。
                        end += 1;
                    } else {
                        // 数据仍不完整，等下一批输出。
                        break String::new();
                    }
                }
            }
        };

        OutputChunk {
            cursor: start + end as u64,
            text,
            eof: self.eof,
            truncated,
        }
    }
}

/// 后台读取一条管道，写入共享缓冲区；结束时登记读取任务完成。
fn spawn_reader<R>(reader: Option<R>, output: Arc<Mutex<SessionOutput>>) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Some(mut reader) = reader {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => output.lock().await.push(&buffer[..read]),
                }
            }
        }
        output.lock().await.finish_reader();
    })
}

/// POSIX Shell 单引号转义，避免文件路径中的特殊字符被解释。
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use umt_protocol_api::ProtocolProvider;

    use crate::provider::{WslProvider, test_distribution};

    fn capabilities() -> CapabilitySet {
        WslProvider::from_distributions(Vec::new())
            .descriptor()
            .capabilities
            .clone()
    }

    fn session_config(distribution: &str) -> WslConfig {
        let json = format!(r#"{{"distribution":"{distribution}"}}"#);
        WslConfig::from_config(&crate::test_config(&json)).expect("配置应通过校验")
    }

    fn start_session(distribution: &str) -> WslSession {
        WslSession::spawn(capabilities(), session_config(distribution), 24, 80)
            .expect("应能启动 wsl.exe")
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    /// 按字节分块读取，包含 NUL 时也不能被截断或转码。
    #[tokio::test]
    async fn read_file_returns_chunked_bytes() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        let path = "/tmp/umt-wsl-read-file";

        let written = session
            .exec(ExecRequest::new(format!(
                "printf 'umt-wsl\\000tail' > {path}"
            )))
            .await
            .expect("写入测试文件失败");
        assert_eq!(written.exit_code, Some(0), "写入失败：{}", written.stderr);

        let first = session
            .read_file(FileReadRequest::new(path, 0, 5))
            .await
            .expect("读取第一块失败");
        assert_eq!(first.offset, 0);
        assert_eq!(first.next_offset, 5);
        assert_eq!(first.size, Some(12));
        assert!(!first.eof);
        assert_eq!(first.data, b"umt-w");

        let second = session
            .read_file(FileReadRequest::new(path, 5, 1024))
            .await
            .expect("读取第二块失败");
        assert_eq!(second.offset, 5);
        assert_eq!(second.next_offset, 12);
        assert_eq!(second.size, Some(12));
        assert!(second.eof);
        assert_eq!(second.data, b"sl\0tail");

        let beyond = session
            .read_file(FileReadRequest::new(path, 100, 16))
            .await
            .expect("越界读取失败");
        assert_eq!(beyond.offset, 100);
        assert_eq!(beyond.next_offset, 100);
        assert!(beyond.eof);
        assert!(beyond.data.is_empty());

        let _ = session
            .exec(ExecRequest::new(format!("rm -f {path}")))
            .await;
        Box::new(session).close().await.expect("关闭失败");
    }

    #[tokio::test]
    async fn read_file_reports_missing_file() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        let error = session
            .read_file(FileReadRequest::new("/nonexistent-umt-read-file", 0, 16))
            .await
            .unwrap_err();
        Box::new(session).close().await.expect("关闭失败");

        assert!(
            error.to_string().contains("打开 WSL 文件失败"),
            "缺失文件应返回明确错误，实际：{error}"
        );
    }

    #[tokio::test]
    async fn interactive_session_echoes_command_output() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        session.write(b"echo umt-wsl-ok\n").await.expect("写入失败");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = 0;
        let mut output = String::new();
        while Instant::now() < deadline && !output.contains("umt-wsl-ok") {
            let chunk = session.read(cursor, 8192).await.expect("读取失败");
            cursor = chunk.cursor;
            output.push_str(&chunk.text);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Box::new(session).close().await.expect("关闭失败");
        assert!(
            output.contains("umt-wsl-ok"),
            "交互式会话未返回预期输出：{output:?}"
        );
    }

    #[tokio::test]
    async fn exec_runs_echo_inside_wsl() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        let result = session
            .exec(ExecRequest::new("echo umt-wsl-ok"))
            .await
            .expect("执行失败");
        Box::new(session).close().await.expect("关闭失败");

        assert_eq!(result.state, ExecutionState::Succeeded);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "umt-wsl-ok");
        assert!(result.stderr.trim().is_empty());
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn exec_reports_failure_exit_code_and_stderr() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        let result = session
            .exec(ExecRequest::new("echo umt-boom >&2; exit 3"))
            .await
            .expect("执行失败");
        Box::new(session).close().await.expect("关闭失败");

        assert_eq!(result.state, ExecutionState::Failed);
        assert_eq!(result.exit_code, Some(3));
        assert!(result.stderr.contains("umt-boom"));
    }

    #[tokio::test]
    async fn exec_times_out_and_kills_process() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        let request = ExecRequest {
            timeout_ms: 500,
            ..ExecRequest::new("sleep 30")
        };
        let result = session
            .exec(request)
            .await
            .expect("超时应返回结果而不是错误");
        Box::new(session).close().await.expect("关闭失败");

        assert_eq!(result.state, ExecutionState::TimedOut);
        assert!(result.duration_ms < 10_000, "超时后应立即返回");
    }

    #[tokio::test]
    async fn resize_is_recorded_and_never_fails() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let mut session = start_session(&distribution);
        session.resize(40, 120).await.expect("resize 应返回 Ok");
        Box::new(session).close().await.expect("关闭失败");
    }

    /// 关闭会话后 wsl.exe 进程必须消失（Job Object + kill 兜底），不能泄漏。
    #[cfg(windows)]
    #[tokio::test]
    async fn closing_session_kills_child_process() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let session = start_session(&distribution);
        let pid = session.pid().expect("会话应持有子进程");
        Box::new(session).close().await.expect("关闭失败");

        let mut alive = true;
        for _ in 0..30 {
            if !process_alive(pid) {
                alive = false;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !alive,
            "关闭会话后 wsl.exe 子进程（PID {pid}）仍然存活，存在进程泄漏"
        );
    }

    /// 用 Drop（而不是 close）释放会话时也不能泄漏子进程。
    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_session_kills_child_process() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let session = start_session(&distribution);
        let pid = session.pid().expect("会话应持有子进程");
        drop(session);

        let mut alive = true;
        for _ in 0..30 {
            if !process_alive(pid) {
                alive = false;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !alive,
            "Drop 会话后 wsl.exe 子进程（PID {pid}）仍然存活，存在进程泄漏"
        );
    }

    #[cfg(windows)]
    fn process_alive(pid: u32) -> bool {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output();
        match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()),
            // 无法检查时不误判为泄漏。
            Err(_) => false,
        }
    }
}
