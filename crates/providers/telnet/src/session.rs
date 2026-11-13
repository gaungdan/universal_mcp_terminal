//! Telnet 会话：登录状态机、增量读取、命令执行与分页自动翻页。

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use umt_core::{Capability, CapabilitySet, ExecutionState, SessionId};
use umt_protocol_api::session::{ExecRequest, ExecResult, OutputChunk, TerminalSession};
use umt_protocol_api::{ProviderError, SessionError};

use crate::config::TelnetConfig;
use crate::wire::{TelnetDecoder, detect_pager};

/// 自动翻页后写入输出缓冲的标记，便于用户与审计追溯。
const PAGER_MARKER: &[u8] = b"\r\n[umt: pager prompt detected, sent SPACE]\r\n";
/// `read` 在没有待消费数据时最多等待新数据的时间。
const READ_FILL_WAIT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginStage {
    /// 等待登录提示或直接出现命令提示符。
    LoginOrCommand,
    /// 已发送用户名，等待密码提示或直接出现命令提示符。
    PasswordOrCommand,
    /// 已发送密码，等待命令提示符。
    Command,
}

/// 已建立的 Telnet 会话。
pub struct TelnetSession {
    id: SessionId,
    capabilities: CapabilitySet,
    config: TelnetConfig,
    reader: Option<ReadHalf<TcpStream>>,
    writer: Option<WriteHalf<TcpStream>>,
    decoder: TelnetDecoder,
    /// 保留完整历史输出，`cursor` 为调用方已消费的字节数。
    buffer: Vec<u8>,
    /// 分页扫描起点，避免对同一分页提示重复发送空格。
    pager_scan_from: usize,
    eof: bool,
}

impl TelnetSession {
    /// 建立 TCP 连接并完成登录握手。
    pub(crate) async fn open(config: TelnetConfig, id: SessionId) -> Result<Self, ProviderError> {
        let stream = connect_tcp(&config).await?;
        let mut session = Self::from_stream(stream, config, id);
        session.login().await?;
        Ok(session)
    }

    pub(crate) fn from_stream(stream: TcpStream, config: TelnetConfig, id: SessionId) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let capabilities = CapabilitySet::of([
            Capability::Shell,
            Capability::Exec,
            Capability::InteractiveTerminal,
        ]);
        Self {
            id,
            capabilities,
            decoder: TelnetDecoder::new(&config.terminal_type),
            config,
            reader: Some(reader),
            writer: Some(writer),
            buffer: Vec::new(),
            pager_scan_from: 0,
            eof: false,
        }
    }

    /// 登录状态机：
    /// 等待登录提示 → 发送用户名 → 等待密码提示 → 发送密码 → 等待命令提示符。
    /// 设备无需登录时，直接出现命令提示符也算成功。
    pub(crate) async fn login(&mut self) -> Result<(), ProviderError> {
        let deadline = Instant::now() + self.config.connection_timeout;
        let mut stage = LoginStage::LoginOrCommand;
        let mut scan_from = 0usize;

        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ProviderError::Timeout(format!(
                    "等待 Telnet 登录提示超时（{} 秒）",
                    self.config.connection_timeout.as_secs()
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            if let Err(err) = self.pump_once(remaining).await {
                return Err(ProviderError::Disconnected(format!(
                    "Telnet 连接在登录过程中断开：{err}"
                )));
            }

            let text = self.text_from(scan_from);
            match stage {
                LoginStage::LoginOrCommand => {
                    if self.config.command_prompt.is_match(&text) {
                        tracing::debug!("Telnet 设备直接出现命令提示符，无需登录");
                        return Ok(());
                    }
                    if self.config.login_prompt.is_match(&text) {
                        let Some(username) = self.config.username.clone() else {
                            return Err(ProviderError::AuthFailed(
                                "设备要求登录，但未配置用户名".into(),
                            ));
                        };
                        self.send_line(&username).await?;
                        scan_from = self.buffer.len();
                        stage = LoginStage::PasswordOrCommand;
                        continue;
                    }
                    if self.config.password_prompt.is_match(&text) {
                        self.send_password(&mut scan_from, &mut stage).await?;
                        continue;
                    }
                }
                LoginStage::PasswordOrCommand => {
                    if self.config.command_prompt.is_match(&text) {
                        return Ok(());
                    }
                    if self.config.password_prompt.is_match(&text) {
                        self.send_password(&mut scan_from, &mut stage).await?;
                        continue;
                    }
                    if self.config.login_prompt.is_match(&text) {
                        return Err(ProviderError::AuthFailed(
                            "设备重新提示登录，用户名可能被拒绝".into(),
                        ));
                    }
                }
                LoginStage::Command => {
                    if self.config.command_prompt.is_match(&text) {
                        return Ok(());
                    }
                    if self.config.login_prompt.is_match(&text)
                        || self.config.password_prompt.is_match(&text)
                    {
                        return Err(ProviderError::AuthFailed(
                            "认证被拒绝，设备重新提示登录".into(),
                        ));
                    }
                }
            }

            if self.eof {
                return Err(ProviderError::Disconnected("Telnet 连接已关闭".into()));
            }
        }
    }

    async fn send_password(
        &mut self,
        scan_from: &mut usize,
        stage: &mut LoginStage,
    ) -> Result<(), ProviderError> {
        let Some(password) = self.config.password.clone() else {
            return Err(ProviderError::AuthFailed(
                "设备要求密码，但未提供 password 机密值".into(),
            ));
        };
        self.send_line(&password).await?;
        *scan_from = self.buffer.len();
        *stage = LoginStage::Command;
        Ok(())
    }

    async fn send_line(&mut self, line: &str) -> Result<(), ProviderError> {
        let mut data = Vec::with_capacity(line.len() + 2);
        data.extend_from_slice(line.as_bytes());
        data.extend_from_slice(b"\r\n");
        self.write_bytes(&data)
            .await
            .map_err(|err| ProviderError::Disconnected(format!("发送登录信息失败：{err}")))
    }

    /// 从 socket 拉取一次数据并写入缓冲；没有数据时由超时返回。
    async fn pump_once(&mut self, wait: Duration) -> Result<(), SessionError> {
        let mut chunk = [0u8; 4096];
        let read_result = {
            let Some(reader) = self.reader.as_mut() else {
                return Err(SessionError::Closed);
            };
            tokio::time::timeout(wait, reader.read(&mut chunk)).await
        };

        match read_result {
            Err(_) => Ok(()),
            Ok(Err(err)) => Err(SessionError::Io(err.to_string())),
            Ok(Ok(0)) => {
                self.eof = true;
                Ok(())
            }
            Ok(Ok(received)) => {
                let mut data = Vec::with_capacity(received);
                let mut response = Vec::new();
                self.decoder
                    .process(&chunk[..received], &mut data, &mut response);
                if !response.is_empty() {
                    self.write_bytes(&response).await?;
                }
                if data.is_empty() {
                    return Ok(());
                }
                self.buffer.extend_from_slice(&data);
                self.handle_pager().await?;
                Ok(())
            }
        }
    }

    /// 检测新输出尾部的分页提示，自动发送空格并记录到输出。
    async fn handle_pager(&mut self) -> Result<(), SessionError> {
        let scan_from = self.pager_scan_from.min(self.buffer.len());
        if scan_from == self.buffer.len() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buffer[scan_from..]).into_owned();
        if !detect_pager(&text) {
            return Ok(());
        }

        self.write_bytes(b" ").await?;
        self.buffer.extend_from_slice(PAGER_MARKER);
        self.pager_scan_from = self.buffer.len();
        tracing::debug!("检测到 Telnet 分页提示，已自动发送空格翻页");
        Ok(())
    }

    fn text_from(&self, start: usize) -> String {
        let start = start.min(self.buffer.len());
        String::from_utf8_lossy(&self.buffer[start..]).into_owned()
    }

    async fn write_bytes(&mut self, data: &[u8]) -> Result<(), SessionError> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(SessionError::Closed);
        };
        writer
            .write_all(data)
            .await
            .map_err(|err| SessionError::Io(err.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|err| SessionError::Io(err.to_string()))
    }

    fn exec_partial(
        &self,
        state: ExecutionState,
        scan_from: usize,
        request: &ExecRequest,
        started: Instant,
    ) -> ExecResult {
        let text = self.text_from(scan_from);
        let (stdout, truncated) = truncate(
            clean_exec_output(&text, &request.command),
            request.max_output_bytes,
        );
        ExecResult {
            state,
            exit_code: None,
            stdout,
            stderr: String::new(),
            duration_ms: elapsed_ms(started),
            truncated,
        }
    }

    /// 释放 socket；`close` 与连接测试共用。
    pub(crate) async fn shutdown(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.shutdown().await;
        }
        self.reader = None;
    }
}

/// 建立 TCP 连接，连接超时映射为 `ProviderError::Timeout`。
pub(crate) async fn connect_tcp(config: &TelnetConfig) -> Result<TcpStream, ProviderError> {
    let address = format!("{}:{}", config.host, config.port);
    let stream = tokio::time::timeout(
        config.connection_timeout,
        TcpStream::connect((config.host.as_str(), config.port)),
    )
    .await
    .map_err(|_| {
        ProviderError::Timeout(format!(
            "连接 {address} 超时（{} 秒）",
            config.connection_timeout.as_secs()
        ))
    })?
    .map_err(|err| ProviderError::Network(format!("连接 {address} 失败：{err}")))?;

    stream
        .set_nodelay(true)
        .map_err(|err| ProviderError::Network(format!("设置 TCP_NODELAY 失败：{err}")))?;
    Ok(stream)
}

#[async_trait::async_trait]
impl TerminalSession for TelnetSession {
    fn session_id(&self) -> SessionId {
        self.id
    }

    fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), SessionError> {
        self.write_bytes(data).await
    }

    async fn read(&mut self, cursor: u64, max_bytes: usize) -> Result<OutputChunk, SessionError> {
        if self.reader.is_none() {
            return Err(SessionError::Closed);
        }
        let requested = usize::try_from(cursor).unwrap_or(usize::MAX);
        if requested >= self.buffer.len() && !self.eof {
            self.pump_once(READ_FILL_WAIT).await?;
        }

        let start = requested.min(self.buffer.len());
        let end = start.saturating_add(max_bytes).min(self.buffer.len());
        Ok(OutputChunk {
            cursor: end as u64,
            text: String::from_utf8_lossy(&self.buffer[start..end]).into_owned(),
            eof: self.eof && end == self.buffer.len(),
            truncated: false,
        })
    }

    async fn resize(&mut self, _rows: u16, _cols: u16) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability(
            Capability::TerminalResize,
        ))
    }

    async fn exec(&mut self, request: ExecRequest) -> Result<ExecResult, SessionError> {
        if self.reader.is_none() {
            return Err(SessionError::Closed);
        }
        let started = Instant::now();

        let mut line = Vec::with_capacity(request.command.len() + 2);
        line.extend_from_slice(request.command.as_bytes());
        line.extend_from_slice(b"\r\n");
        self.write_bytes(&line).await?;

        let scan_from = self.buffer.len();
        let deadline = started + Duration::from_millis(request.timeout_ms.max(1));

        loop {
            let text = self.text_from(scan_from);
            if let Some(found) = self.config.command_prompt.find(&text) {
                let (stdout, truncated) = truncate(
                    clean_exec_output(&text[..found.start()], &request.command),
                    request.max_output_bytes,
                );
                return Ok(ExecResult {
                    state: ExecutionState::Succeeded,
                    exit_code: None,
                    stdout,
                    stderr: String::new(),
                    duration_ms: elapsed_ms(started),
                    truncated,
                });
            }

            if self.eof {
                return Ok(self.exec_partial(
                    ExecutionState::Unknown,
                    scan_from,
                    &request,
                    started,
                ));
            }

            let now = Instant::now();
            if now >= deadline {
                return Ok(self.exec_partial(
                    ExecutionState::TimedOut,
                    scan_from,
                    &request,
                    started,
                ));
            }

            let remaining = deadline.saturating_duration_since(now);
            if self.pump_once(remaining).await.is_err() {
                // 连接中断时命令可能已经在设备上执行，结果只能是 Unknown。
                return Ok(self.exec_partial(
                    ExecutionState::Unknown,
                    scan_from,
                    &request,
                    started,
                ));
            }
        }
    }

    async fn close(mut self: Box<Self>) -> Result<(), SessionError> {
        self.shutdown().await;
        Ok(())
    }
}

/// 去掉设备回显的命令行，并清理尾部换行/空格。
fn clean_exec_output(raw: &str, command: &str) -> String {
    let text = raw.trim_start_matches(['\r', '\n']);
    let body = match text.split_once('\n') {
        Some((first, rest)) if first.trim_end_matches('\r').trim() == command.trim() => rest,
        Some(_) => text,
        None if text.trim() == command.trim() => "",
        None => text,
    };
    body.trim_end_matches(['\r', '\n', ' ']).to_string()
}

fn truncate(mut text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut cut = max_bytes;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    (text, true)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}
