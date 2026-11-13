//! `wsl.exe` 子进程的统一执行助手：超时、输出截断、进程树清理。

use std::io;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::job::JobGuard;

/// `wsl.exe` 的可执行文件名（通过系统 PATH 解析）。
pub(crate) const WSL_EXE: &str = "wsl.exe";

/// 读取输出管道时单个流的最大保留字节数的兜底值。
const DEFAULT_MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// 等待读取任务收尾的上限，避免子进程被杀死后读取任务长期挂起。
const READER_GRACE: Duration = Duration::from_secs(3);

/// 一次 `wsl.exe` 调用的参数。
pub(crate) struct WslCommand {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

/// 一次 `wsl.exe` 调用的结果（输出为原始字节，由上层按需解码）。
pub(crate) struct CommandOutcome {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub truncated: bool,
    pub duration: Duration,
}

/// 构造一个隐藏控制台窗口、并已注入 WSL 环境变量的 `wsl.exe` 命令。
pub(crate) fn build_command(env: &[(String, String)]) -> Command {
    let mut command = Command::new(WSL_EXE);
    hide_console_window(&mut command);
    apply_env(&mut command, env);
    command
}

/// 执行并捕获输出。
///
/// - 超时会终止整个 Job（进程树），并把 `timed_out` 置为 true；
/// - 输出超过 `max_output_bytes` 时继续排空管道但丢弃多余字节，`truncated` 置为 true；
/// - 无论成功与否，返回前都会关闭 Job 句柄，确保没有残留进程。
pub(crate) async fn run_capture(spec: WslCommand) -> io::Result<CommandOutcome> {
    let max_output_bytes = if spec.max_output_bytes == 0 {
        DEFAULT_MAX_OUTPUT_BYTES
    } else {
        spec.max_output_bytes
    };

    let mut command = build_command(&spec.env);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let started = Instant::now();
    let mut child = command.spawn()?;

    // 先把子进程放进 Job，后续超时/关闭时才能连带结束 wsl.exe 启动的整个进程树。
    let job = JobGuard::create();
    if let Some(job) = &job
        && let Some(process) = child_raw_handle(&child)
        && let Err(error) = job.assign(process)
    {
        tracing::warn!(%error, "wsl.exe 子进程加入 Job Object 失败，超时兜底只能结束直接子进程");
    }

    let stdout = spawn_capped_reader(child.stdout.take(), max_output_bytes);
    let stderr = spawn_capped_reader(child.stderr.take(), max_output_bytes);

    let mut timed_out = false;
    let mut exit_code = None;
    match tokio::time::timeout(spec.timeout, child.wait()).await {
        Ok(Ok(status)) => exit_code = status.code(),
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            timed_out = true;
            if let Some(job) = &job {
                job.terminate();
            }
            let _ = child.start_kill();
            let _ = tokio::time::timeout(READER_GRACE, child.wait()).await;
        }
    }

    let (stdout, stdout_truncated) = join_reader(stdout).await;
    let (stderr, stderr_truncated) = join_reader(stderr).await;

    // 关闭 Job 句柄：KILL_ON_JOB_CLOSE 会清掉仍挂在 Job 上的残留进程。
    drop(job);

    Ok(CommandOutcome {
        exit_code,
        stdout,
        stderr,
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
        duration: started.elapsed(),
    })
}

/// 取子进程的原始句柄（Windows 之外恒为 `None`）。
#[cfg(windows)]
pub(crate) fn child_raw_handle(child: &Child) -> Option<usize> {
    child.raw_handle().map(|handle| handle as usize)
}

#[cfg(not(windows))]
pub(crate) fn child_raw_handle(_child: &Child) -> Option<usize> {
    None
}

/// 持续读取一个输出管道，最多保留 `max_bytes` 字节，超出部分丢弃。
fn spawn_capped_reader<R>(reader: Option<R>, max_bytes: usize) -> JoinHandle<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut collected = Vec::new();
        let mut truncated = false;
        let Some(mut reader) = reader else {
            return (collected, truncated);
        };
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    let remaining = max_bytes.saturating_sub(collected.len());
                    let taken = remaining.min(read);
                    collected.extend_from_slice(&buffer[..taken]);
                    if taken < read {
                        truncated = true;
                    }
                }
                Err(_) => break,
            }
        }
        (collected, truncated)
    })
}

/// 收集读取任务结果；超时或任务异常时返回空输出。
async fn join_reader(mut task: JoinHandle<(Vec<u8>, bool)>) -> (Vec<u8>, bool) {
    match tokio::time::timeout(READER_GRACE, &mut task).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => (Vec::new(), false),
        Err(_) => {
            task.abort();
            (Vec::new(), false)
        }
    }
}

/// 注入环境变量，并登记到 `WSLENV` 让 wsl.exe 把它们传进发行版。
///
/// `wsl.exe` 默认不会把 Windows 环境变量带进 Linux；只有列在 `WSLENV` 里的变量才会传递，
/// 因此这里同时设置进程环境与 `WSLENV` 列表（保留父进程已有的 `WSLENV` 内容）。
fn apply_env(command: &mut Command, env: &[(String, String)]) {
    if env.is_empty() {
        return;
    }
    for (key, value) in env {
        command.env(key, value);
    }
    let names = env
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(":");
    let existing = std::env::var("WSLENV").unwrap_or_default();
    let wslenv = if existing.trim().is_empty() {
        names
    } else {
        format!("{}:{names}", existing.trim_end_matches(':'))
    };
    command.env("WSLENV", wslenv);
}

/// 桌面前端是 GUI 进程，创建子进程时禁止弹出控制台窗口。
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}
