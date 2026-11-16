//! SSH 终端会话：交互 shell、增量读取、窗口调整、非交互 exec 与资源释放。

use std::io::SeekFrom;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use russh::client;
use russh::{ChannelMsg, ChannelWriteHalf, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use umt_core::{Capability, CapabilitySet, ExecutionState, SessionId};
use umt_protocol_api::{
    ConnectOptions, ExecRequest, ExecResult, FileChunk, FileReadRequest, OutputChunk,
    ProviderError, SessionError, TerminalSession,
};

use crate::host_key::SshHandler;

/// 内部缓冲上限：超过后丢弃最旧数据并标记 `truncated`，避免无限占用内存。
const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// 后台读取任务向会话投递的事件。
enum SessionEvent {
    Stdout(Bytes),
    Stderr(Bytes),
    ExitStatus,
    Eof,
}

/// 已建立的 SSH 终端会话。
pub struct SshSession {
    id: SessionId,
    capabilities: CapabilitySet,
    handle: Option<client::Handle<SshHandler>>,
    write_half: Option<ChannelWriteHalf<client::Msg>>,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    /// 已接收但尚未交付给调用方的字节。
    buffer: Vec<u8>,
    /// 已交付的字节总数，作为 `OutputChunk.cursor`。
    delivered: u64,
    eof: bool,
    dropped: bool,
    closed: bool,
}

impl SshSession {
    /// 打开会话通道，申请 PTY 与交互 shell，并启动后台读取任务。
    pub(crate) async fn open(
        handle: client::Handle<SshHandler>,
        options: &ConnectOptions,
        terminal_type: &str,
        working_dir: Option<&str>,
        capabilities: CapabilitySet,
    ) -> Result<Self, ProviderError> {
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|err| ProviderError::Io(format!("打开 SSH 会话通道失败：{err}")))?;

        channel
            .request_pty(
                false,
                terminal_type,
                options.cols as u32,
                options.rows as u32,
                0,
                0,
                &[],
            )
            .await
            .map_err(|err| ProviderError::Io(format!("申请 SSH 伪终端失败：{err}")))?;
        channel
            .request_shell(false)
            .await
            .map_err(|err| ProviderError::Io(format!("启动 SSH 交互 Shell 失败：{err}")))?;

        // 交互式 Shell 启动后无法通过 exec 设置工作目录，只能在 Shell 内执行 cd。
        if let Some(dir) = working_dir {
            let command = format!("cd -- {}\n", shell_quote(dir));
            channel
                .data_bytes(Bytes::from(command.into_bytes()))
                .await
                .map_err(|err| ProviderError::Io(format!("设置默认工作目录失败：{err}")))?;
        }

        let (mut read_half, write_half) = channel.split();
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(message) = read_half.wait().await {
                match message {
                    ChannelMsg::Data { data } => {
                        let _ = sender.send(SessionEvent::Stdout(data));
                    }
                    ChannelMsg::ExtendedData { data, ext: 1 } => {
                        let _ = sender.send(SessionEvent::Stderr(data));
                    }
                    ChannelMsg::ExitStatus { .. } => {
                        let _ = sender.send(SessionEvent::ExitStatus);
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => {
                        let _ = sender.send(SessionEvent::Eof);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            id: SessionId::new(),
            capabilities,
            handle: Some(handle),
            write_half: Some(write_half),
            events: receiver,
            buffer: Vec::new(),
            delivered: 0,
            eof: false,
            dropped: false,
            closed: false,
        })
    }

    /// 把后台任务已收到的事件并入缓冲区；不等待新数据。
    async fn drain_events(&mut self) {
        // 让出一次执行机会，确保后台读取任务有机会投递本轮已到达的数据。
        tokio::task::yield_now().await;
        loop {
            match self.events.try_recv() {
                Ok(SessionEvent::Stdout(data)) | Ok(SessionEvent::Stderr(data)) => {
                    push_output(&mut self.buffer, &data);
                }
                Ok(SessionEvent::ExitStatus) => {}
                Ok(SessionEvent::Eof) => self.eof = true,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.eof = true;
                    break;
                }
            }
            if self.buffer.len() > MAX_BUFFER_BYTES {
                let overflow = self.buffer.len() - MAX_BUFFER_BYTES;
                self.buffer.drain(..overflow);
                self.dropped = true;
            }
        }
    }
}

#[async_trait]
impl TerminalSession for SshSession {
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
        let write_half = self.write_half.as_ref().ok_or(SessionError::Closed)?;
        write_half
            .data_bytes(Bytes::copy_from_slice(data))
            .await
            .map_err(|err| SessionError::Io(format!("写入 SSH 通道失败：{err}")))
    }

    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError> {
        if cursor > self.delivered {
            return Err(SessionError::Internal(format!(
                "读取游标 {cursor} 超过已交付位置 {}",
                self.delivered
            )));
        }
        self.drain_events().await;

        let take = max_bytes.min(self.buffer.len());
        let chunk: Vec<u8> = self.buffer.drain(..take).collect();
        self.delivered += take as u64;
        let text = String::from_utf8_lossy(&chunk).into_owned();

        Ok(OutputChunk {
            cursor: self.delivered,
            text,
            eof: self.eof && self.buffer.is_empty(),
            truncated: std::mem::take(&mut self.dropped),
        })
    }

    async fn resize(&mut self, rows: u16, cols: u16) -> Result<(), SessionError> {
        if !self.capabilities.contains(Capability::TerminalResize) {
            return Err(SessionError::UnsupportedCapability(
                Capability::TerminalResize,
            ));
        }
        let write_half = self.write_half.as_ref().ok_or(SessionError::Closed)?;
        write_half
            .window_change(cols as u32, rows as u32, 0, 0)
            .await
            .map_err(|err| SessionError::Io(format!("调整 SSH 终端尺寸失败：{err}")))
    }

    async fn read_file(&mut self, request: FileReadRequest) -> Result<FileChunk, SessionError> {
        if self.closed {
            return Err(SessionError::Closed);
        }
        if !self.capabilities.contains(Capability::FileRead) {
            return Err(SessionError::UnsupportedCapability(Capability::FileRead));
        }
        let handle = self.handle.as_ref().ok_or(SessionError::Closed)?;
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|err| SessionError::Io(format!("打开 SFTP 通道失败：{err}")))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|err| SessionError::Io(format!("启动 SFTP 子系统失败：{err}")))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|err| SessionError::Io(format!("初始化 SFTP 会话失败：{err}")))?;
        let mut file = sftp
            .open(request.path.clone())
            .await
            .map_err(|err| SessionError::Io(format!("打开远程文件失败：{err}")))?;
        let metadata = file
            .metadata()
            .await
            .map_err(|err| SessionError::Io(format!("读取文件元数据失败：{err}")))?;
        let size = metadata.size;
        let max_bytes = request.max_bytes.max(1);

        if let Some(size) = size
            && request.offset > size
        {
            file.close()
                .await
                .map_err(|err| SessionError::Io(format!("关闭远程文件失败：{err}")))?;
            return Ok(FileChunk {
                path: request.path,
                offset: request.offset,
                next_offset: request.offset,
                size: Some(size),
                eof: true,
                data: Vec::new(),
            });
        }

        file.seek(SeekFrom::Start(request.offset))
            .await
            .map_err(|err| SessionError::Io(format!("定位远程文件失败：{err}")))?;
        let mut data = vec![0u8; max_bytes];
        let read = file
            .read(&mut data)
            .await
            .map_err(|err| SessionError::Io(format!("读取远程文件失败：{err}")))?;
        data.truncate(read);
        let next_offset = request.offset + read as u64;
        let eof = match size {
            Some(size) => next_offset >= size,
            None => read < max_bytes,
        };
        file.close()
            .await
            .map_err(|err| SessionError::Io(format!("关闭远程文件失败：{err}")))?;
        let _ = sftp.close().await;

        Ok(FileChunk {
            path: request.path,
            offset: request.offset,
            next_offset,
            size,
            eof,
            data,
        })
    }

    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        let handle = self.handle.as_ref().ok_or(SessionError::Closed)?;
        let ExecRequest {
            command,
            working_dir,
            timeout_ms,
            max_output_bytes,
        } = request;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|err| SessionError::Io(format!("打开 SSH exec 通道失败：{err}")))?;

        let full_command = match working_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(dir) => format!("cd -- {} && {command}", shell_quote(dir)),
            None => command,
        };
        channel
            .exec(true, full_command)
            .await
            .map_err(|err| SessionError::Io(format!("启动 SSH 命令失败：{err}")))?;

        let started = Instant::now();
        let deadline = started + Duration::from_millis(timeout_ms.max(1));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut stdout_truncated = false;
        let mut stderr_truncated = false;
        let mut exit_code = None;
        let mut timed_out = false;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                timed_out = true;
                break;
            }
            match tokio::time::timeout(remaining, channel.wait()).await {
                Err(_) => {
                    timed_out = true;
                    break;
                }
                Ok(None) => break,
                Ok(Some(message)) => match message {
                    ChannelMsg::Data { data } => {
                        push_limited(&mut stdout, &data, max_output_bytes, &mut stdout_truncated)
                    }
                    ChannelMsg::ExtendedData { data, ext: 1 } => {
                        push_limited(&mut stderr, &data, max_output_bytes, &mut stderr_truncated)
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = i32::try_from(exit_status).ok();
                    }
                    // 收到 Eof 且已有退出码时说明命令结束，无需等待 Close 或超时。
                    ChannelMsg::Eof => {
                        if exit_code.is_some() {
                            break;
                        }
                    }
                    ChannelMsg::Close => break,
                    _ => {}
                },
            }
        }
        let _ = channel.close().await;

        let state = if timed_out {
            ExecutionState::TimedOut
        } else {
            match exit_code {
                Some(0) => ExecutionState::Succeeded,
                Some(_) => ExecutionState::Failed,
                None => ExecutionState::Unknown,
            }
        };
        Ok(ExecResult {
            state,
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            duration_ms: started.elapsed().as_millis() as u64,
            truncated: stdout_truncated || stderr_truncated,
        })
    }

    async fn close(mut self: Box<Self>) -> Result<(), SessionError> {
        self.closed = true;
        if let Some(write_half) = self.write_half.take() {
            let _ = write_half.eof().await;
            let _ = write_half.close().await;
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
        }
        Ok(())
    }
}

/// 追加输出，超过上限时截断并标记。
fn push_limited(target: &mut Vec<u8>, data: &[u8], limit: usize, truncated: &mut bool) {
    if target.len() >= limit {
        if !data.is_empty() {
            *truncated = true;
        }
        return;
    }
    let remaining = limit - target.len();
    if data.len() > remaining {
        target.extend_from_slice(&data[..remaining]);
        *truncated = true;
    } else {
        target.extend_from_slice(data);
    }
}

/// 追加到内部缓冲，不做截断（上限由调用方统一处理）。
fn push_output(target: &mut Vec<u8>, data: &[u8]) {
    target.extend_from_slice(data);
}

/// POSIX Shell 单引号转义，避免工作目录中的特殊字符被解释。
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

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn push_limited_truncates_and_marks() {
        let mut buffer = Vec::new();
        let mut truncated = false;
        push_limited(&mut buffer, b"abcdef", 4, &mut truncated);
        assert_eq!(buffer, b"abcd");
        assert!(truncated);

        push_limited(&mut buffer, b"x", 4, &mut truncated);
        assert_eq!(buffer, b"abcd");
        assert!(truncated);
    }
}
