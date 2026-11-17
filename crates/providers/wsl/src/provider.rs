//! WSL Provider：发行版枚举、配置 Schema、真实连接测试与会话创建。

use std::time::{Duration, Instant};

use async_trait::async_trait;
use umt_core::{Capability, CapabilitySet, ConnectionConfig};
use umt_protocol_api::error::ProviderError;
use umt_protocol_api::provider::{ProtocolDescriptor, ProtocolProvider};
use umt_protocol_api::schema::ConfigSchema;
use umt_protocol_api::session::{ConnectOptions, TerminalSession};
use umt_protocol_api::test_result::{ConnectionTestResult, TestStep};

use crate::config::{self, WslConfig};
use crate::decode;
use crate::process::{self, WslCommand};
use crate::session::WslSession;

/// 枚举本机已安装的 WSL 发行版。
///
/// Windows 上执行 `wsl.exe -l -q`，其输出为 UTF-16LE，这里统一解码并过滤空行。
/// 未安装 WSL 或未安装任何发行版时返回空列表（前端据此提示用户先安装发行版）；
/// 非 Windows 平台返回 [`ProviderError::Internal`]。
pub fn list_distributions() -> Result<Vec<String>, ProviderError> {
    #[cfg(not(windows))]
    {
        Err(ProviderError::Internal("仅支持 Windows".into()))
    }

    #[cfg(windows)]
    {
        use std::io::Read;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let mut child = Command::new(process::WSL_EXE)
            .args(["-l", "-q"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| ProviderError::Io(format!("无法执行 wsl.exe：{error}")))?;

        // wsl.exe 首次调用可能初始化子系统，这里轮询等待并设上限，避免调用方被无限阻塞。
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProviderError::Timeout(
                        "枚举 WSL 发行版超时（10 秒）".into(),
                    ));
                }
                Err(error) => {
                    return Err(ProviderError::Io(format!("等待 wsl.exe 退出失败：{error}")));
                }
            }
        }

        // `-l -q` 的输出只有发行版名称，管道缓冲区足够，进程退出后再读取不会死锁。
        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout)
                .map_err(|error| ProviderError::Io(format!("读取 wsl.exe 输出失败：{error}")))?;
        }
        let status = child
            .wait()
            .map_err(|error| ProviderError::Io(format!("等待 wsl.exe 退出失败：{error}")))?;

        if !status.success() {
            // 未安装 WSL / 未安装发行版时 wsl.exe 会返回非零码并输出提示信息。
            tracing::debug!("wsl.exe -l -q 返回非零退出码，视为没有可用发行版");
            return Ok(Vec::new());
        }

        let mut distributions: Vec<String> = Vec::new();
        for name in decode::split_lines(&decode::decode_output(&stdout)) {
            if !distributions.contains(&name) {
                distributions.push(name);
            }
        }
        Ok(distributions)
    }
}

/// WSL 协议 Provider。
///
/// 配置 Schema 中的发行版候选来自构造时的枚举结果；发行版列表变化后可用
/// [`WslProvider::from_distributions`] 重新构造（例如用户刚安装了新发行版）。
#[derive(Debug)]
pub struct WslProvider {
    descriptor: ProtocolDescriptor,
    schema: ConfigSchema,
}

impl Default for WslProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WslProvider {
    /// 枚举本机发行版后构造；枚举失败时退化为文本输入。
    pub fn new() -> Self {
        let distributions = list_distributions().unwrap_or_else(|error| {
            tracing::debug!(%error, "枚举 WSL 发行版失败，配置 Schema 回退为文本输入");
            Vec::new()
        });
        Self::from_distributions(distributions)
    }

    /// 使用指定的发行版候选构造（便于前端在安装新发行版后刷新 Schema）。
    pub fn from_distributions(mut distributions: Vec<String>) -> Self {
        distributions.retain(|name| !name.trim().is_empty());
        Self {
            descriptor: ProtocolDescriptor {
                protocol_id: "wsl".into(),
                display_name: "WSL".into(),
                version: "1".into(),
                description:
                    "Windows Subsystem for Linux：通过 wsl.exe 在发行版内执行命令与交互式 Shell"
                        .into(),
                capabilities: CapabilitySet::of([
                    Capability::Shell,
                    Capability::Exec,
                    Capability::InteractiveTerminal,
                    Capability::TerminalResize,
                    Capability::EnvironmentInfo,
                    Capability::FileRead,
                ]),
            },
            schema: config::schema(&distributions),
        }
    }
}

#[async_trait]
impl ProtocolProvider for WslProvider {
    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn config_schema(&self) -> &ConfigSchema {
        &self.schema
    }

    /// 在默认校验（结构 + Schema）之上增加语义校验，并放宽发行版字段。
    async fn validate_config(&self, config: &ConnectionConfig) -> Result<(), ProviderError> {
        if config.protocol_id != self.descriptor.protocol_id {
            return Err(ProviderError::ConfigInvalid {
                field: "protocol_id".into(),
                message: format!(
                    "配置协议 {} 与 Provider {} 不一致",
                    config.protocol_id, self.descriptor.protocol_id
                ),
            });
        }
        config
            .validate_shape()
            .map_err(|message| ProviderError::ConfigInvalid {
                field: "<root>".into(),
                message,
            })?;

        if let Err(error) = self.schema.validate(&config.values) {
            // Schema 的 Select 候选来自启动时的枚举结果，可能落后于实际安装情况；
            // 发行版字段只要是非空字符串就放行，最终由 wsl.exe 判定是否存在。
            let stale_distribution = matches!(
                &error,
                ProviderError::ConfigInvalid { field, .. } if field == "distribution"
            );
            if !stale_distribution {
                return Err(error);
            }
        }

        // 复用 from_config 做默认值、取值范围与环境变量格式校验。
        WslConfig::from_config(config)?;
        Ok(())
    }

    async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionTestResult, ProviderError> {
        self.validate_config(config).await?;
        let settings = WslConfig::from_config(config)?;

        let outcome = process::run_capture(WslCommand {
            args: settings.exec_args("uname -a", settings.working_dir.as_deref()),
            env: settings.env.clone(),
            timeout: settings.timeout,
            max_output_bytes: 64 * 1024,
        })
        .await
        .map_err(|error| ProviderError::Io(format!("无法执行 wsl.exe：{error}")))?;

        let latency_ms = outcome.duration.as_millis() as u64;
        if outcome.timed_out {
            return Err(ProviderError::Timeout(format!(
                "WSL 环境启动超时（{} 秒）",
                settings.timeout.as_secs()
            )));
        }

        let stdout = decode::decode_output(&outcome.stdout);
        let stderr = decode::decode_output(&outcome.stderr);
        if outcome.exit_code != Some(0) {
            let detail = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            return Err(classify_failure(
                &settings.distribution,
                outcome.exit_code,
                &detail,
            ));
        }

        let kernel = stdout.lines().next().unwrap_or_default().trim();
        let message = if kernel.is_empty() {
            format!("WSL 环境启动成功（{}）", settings.distribution)
        } else {
            format!("WSL 环境启动成功（{}）：{kernel}", settings.distribution)
        };

        Ok(ConnectionTestResult::success(
            message,
            latency_ms,
            vec![
                TestStep::passed("校验配置", None),
                TestStep::passed("启动 WSL 环境并执行 uname -a", Some(latency_ms)),
            ],
        ))
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
        options: ConnectOptions,
    ) -> Result<Box<dyn TerminalSession>, ProviderError> {
        self.validate_config(config).await?;
        let mut settings = WslConfig::from_config(config)?;

        // ConnectOptions 中的工作目录与环境变量覆盖连接配置里的默认值。
        if let Some(directory) = options
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            settings.working_dir = Some(directory.to_string());
        }
        for (key, value) in options.env {
            settings.set_env(key, value);
        }

        let session = WslSession::spawn(
            self.descriptor.capabilities.clone(),
            settings,
            options.rows,
            options.cols,
        )?;
        Ok(Box::new(session))
    }
}

/// 把退出码与错误输出整理成对用户/前端有意义的错误。
fn classify_failure(distribution: &str, exit_code: Option<i32>, detail: &str) -> ProviderError {
    let lower = detail.to_ascii_lowercase();
    let missing = lower.contains("wsl_e_distro_not_found")
        || lower.contains("no distribution")
        || detail.contains("不存在")
        || detail.contains("没有找到");
    if missing {
        ProviderError::ConfigInvalid {
            field: "distribution".into(),
            message: format!("发行版 {distribution} 不存在或未安装：{detail}"),
        }
    } else {
        let code = exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "未知".to_string());
        ProviderError::Internal(format!("WSL 环境启动失败（退出码 {code}）：{detail}"))
    }
}

/// 测试用：取本机第一个发行版，没有则返回 `None`（相关用例运行期跳过）。
#[cfg(test)]
pub(crate) fn test_distribution() -> Option<String> {
    #[cfg(windows)]
    {
        list_distributions()
            .ok()
            .and_then(|distributions| distributions.into_iter().next())
    }

    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_protocol_api::schema::FieldKind;

    fn provider() -> WslProvider {
        WslProvider::from_distributions(vec!["Ubuntu".to_string(), "Debian".to_string()])
    }

    #[test]
    fn descriptor_declares_wsl_capabilities() {
        let provider = provider();
        let descriptor = provider.descriptor();
        assert_eq!(descriptor.protocol_id, "wsl");
        assert_eq!(descriptor.display_name, "WSL");
        assert_eq!(descriptor.version, "1");
        for capability in [
            Capability::Shell,
            Capability::Exec,
            Capability::InteractiveTerminal,
            Capability::TerminalResize,
            Capability::EnvironmentInfo,
            Capability::FileRead,
        ] {
            assert!(
                descriptor.capabilities.contains(capability),
                "缺少能力 {}",
                capability.as_str()
            );
        }
        assert_eq!(descriptor.capabilities.len(), 6);
    }

    #[test]
    fn schema_uses_expected_keys_and_kinds() {
        let provider = provider();
        let schema = provider.config_schema();
        let keys: Vec<&str> = schema
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![
                "distribution",
                "working_dir",
                "shell",
                "startup_args",
                "process_timeout_secs",
                "env"
            ]
        );
        assert!(matches!(
            schema.field("distribution").map(|field| &field.kind),
            Some(FieldKind::Select { options }) if options.len() == 2
        ));
        assert!(matches!(
            schema.field("env").map(|field| &field.kind),
            Some(FieldKind::Multiline)
        ));
        let timeout = schema.field("process_timeout_secs").expect("应有超时字段");
        assert_eq!(timeout.default_value, Some(30.into()));
        assert_eq!(
            timeout.validation.as_ref().and_then(|rule| rule.min),
            Some(1)
        );
        assert_eq!(
            timeout.validation.as_ref().and_then(|rule| rule.max),
            Some(3600)
        );
        assert_eq!(
            schema
                .field("shell")
                .and_then(|field| field.default_value.clone()),
            Some("/bin/bash".into())
        );
    }

    #[test]
    fn schema_falls_back_to_text_without_distributions() {
        let provider = WslProvider::from_distributions(Vec::new());
        assert!(matches!(
            provider
                .config_schema()
                .field("distribution")
                .map(|field| &field.kind),
            Some(FieldKind::Text)
        ));
    }

    #[tokio::test]
    async fn rejects_missing_distribution() {
        let error = provider()
            .validate_config(&crate::test_config(r#"{"shell":"/bin/bash"}"#))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::ConfigInvalid { ref field, .. } if field == "distribution"
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_field() {
        let error = provider()
            .validate_config(&crate::test_config(
                r#"{"distribution":"Ubuntu","host":"127.0.0.1"}"#,
            ))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::ConfigInvalid { ref field, .. } if field == "host"
        ));
    }

    #[tokio::test]
    async fn rejects_timeout_out_of_range() {
        for seconds in [0, 3601] {
            let error = provider()
                .validate_config(&crate::test_config(&format!(
                    r#"{{"distribution":"Ubuntu","process_timeout_secs":{seconds}}}"#
                )))
                .await
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    ProviderError::ConfigInvalid { ref field, .. } if field == "process_timeout_secs"
                ),
                "超时 {seconds} 秒应被拒绝"
            );
        }
    }

    #[tokio::test]
    async fn accepts_distribution_not_in_options() {
        provider()
            .validate_config(&crate::test_config(r#"{"distribution":"ArchLinux"}"#))
            .await
            .expect("Schema 候选过期时也应接受任意发行版名");
    }

    #[tokio::test]
    async fn rejects_illegal_env_line() {
        let error = provider()
            .validate_config(&crate::test_config(
                r#"{"distribution":"Ubuntu","env":"NO_EQUALS"}"#,
            ))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::ConfigInvalid { ref field, .. } if field == "env"
        ));
    }

    #[test]
    fn list_distributions_does_not_panic() {
        match list_distributions() {
            Ok(distributions) => assert!(
                distributions
                    .iter()
                    .all(|name| !name.trim().is_empty() && !name.contains('\0')),
                "发行版名称不应为空或包含 NUL：{distributions:?}"
            ),
            Err(error) => {
                // 非 Windows 或本机 wsl.exe 不可用：记录即可，不允许 panic。
                eprintln!("本机无法枚举 WSL 发行版：{error}");
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_connection_succeeds_for_installed_distribution() {
        let Some(distribution) = test_distribution() else {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        };
        let settings = WslProvider::from_distributions(vec![distribution.clone()]);
        let config = crate::test_config(&format!(r#"{{"distribution":"{distribution}"}}"#));
        let result = settings
            .test_connection(&config)
            .await
            .expect("已安装的发行版应能通过连接测试");
        assert!(result.ok);
        assert!(result.message.contains("WSL 环境启动成功"));
        assert!(result.latency_ms.is_some());
        assert_eq!(result.steps.len(), 2);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_connection_rejects_missing_distribution() {
        if test_distribution().is_none() {
            eprintln!("跳过：本机没有可用的 WSL 发行版");
            return;
        }
        let settings = WslProvider::from_distributions(vec!["umt-missing-distro".to_string()]);
        let error = settings
            .test_connection(&crate::test_config(
                r#"{"distribution":"umt-missing-distro"}"#,
            ))
            .await
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::ConfigInvalid { ref field, .. } if field == "distribution"),
            "不存在的发行版应返回 distribution 字段错误，实际：{error:?}"
        );
    }
}
