//! WSL 连接配置：Schema 声明、字段解析与语义校验。

use std::time::Duration;

use umt_core::ConnectionConfig;
use umt_protocol_api::error::ProviderError;
use umt_protocol_api::schema::{
    ConfigField, ConfigSchema, FieldKind, FieldValidation, SelectOption,
};

pub(crate) const DEFAULT_SHELL: &str = "/bin/bash";
pub(crate) const DEFAULT_TIMEOUT_SECS: i64 = 30;
const MIN_TIMEOUT_SECS: i64 = 1;
const MAX_TIMEOUT_SECS: i64 = 3600;

/// 解析后的 WSL 连接配置（Schema 之外的默认值与语义归一化结果）。
#[derive(Debug, Clone)]
pub(crate) struct WslConfig {
    pub distribution: String,
    pub working_dir: Option<String>,
    pub shell: String,
    /// 已按空白拆分好的 shell 启动参数，例如 `-lc`。
    pub startup_args: Vec<String>,
    pub timeout: Duration,
    pub env: Vec<(String, String)>,
}

impl WslConfig {
    /// 从连接配置解析，所有字段都已做默认值与合法性处理。
    pub(crate) fn from_config(config: &ConnectionConfig) -> Result<Self, ProviderError> {
        let distribution = config
            .get_str("distribution")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ProviderError::ConfigInvalid {
                field: "distribution".into(),
                message: "发行版名称不能为空".into(),
            })?
            .to_string();

        let working_dir = config
            .get_str("working_dir")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let shell = config
            .get_str("shell")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_SHELL)
            .to_string();

        let startup_args = config
            .get_str("startup_args")
            .map(|args| args.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();

        let seconds = config
            .values
            .get("process_timeout_secs")
            .and_then(|value| value.as_i64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&seconds) {
            return Err(ProviderError::ConfigInvalid {
                field: "process_timeout_secs".into(),
                message: format!(
                    "进程超时必须介于 {MIN_TIMEOUT_SECS} 和 {MAX_TIMEOUT_SECS} 秒之间"
                ),
            });
        }

        let env = parse_env(config.get_str("env").unwrap_or_default()).map_err(|message| {
            ProviderError::ConfigInvalid {
                field: "env".into(),
                message,
            }
        })?;

        Ok(Self {
            distribution,
            working_dir,
            shell,
            startup_args,
            timeout: Duration::from_secs(seconds as u64),
            env,
        })
    }

    /// 写入一个环境变量，同名时覆盖。
    pub(crate) fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.env.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value,
            None => self.env.push((key, value)),
        }
    }

    /// exec / 连接测试使用的 shell 启动参数；未配置时默认 `-lc`，
    /// 保证命令字符串作为一个参数交给 shell 执行。
    pub(crate) fn exec_shell_args(&self) -> Vec<String> {
        if self.startup_args.is_empty() {
            vec!["-lc".to_string()]
        } else {
            self.startup_args.clone()
        }
    }

    /// 公共前缀：`wsl.exe -d <distro> [--cd <dir>] -- <shell>`。
    pub(crate) fn base_args(&self, working_dir: Option<&str>) -> Vec<String> {
        let mut args = vec!["-d".to_string(), self.distribution.clone()];
        if let Some(directory) = working_dir {
            args.push("--cd".to_string());
            args.push(directory.to_string());
        }
        args.push("--".to_string());
        args.push(self.shell.clone());
        args
    }

    /// exec 参数：`wsl.exe -d <distro> [--cd <dir>] -- <shell> [startup_args] "<command>"`。
    pub(crate) fn exec_args(&self, command: &str, working_dir: Option<&str>) -> Vec<String> {
        let mut args = self.base_args(working_dir);
        args.extend(self.exec_shell_args());
        args.push(command.to_string());
        args
    }

    /// 交互式终端参数：`wsl.exe -d <distro> [--cd <dir>] -- <shell>`。
    ///
    /// 不追加 `startup_args`：`-lc` 这类参数需要命令，交互式 shell 追加后会立即报错退出。
    pub(crate) fn interactive_args(&self) -> Vec<String> {
        self.base_args(self.working_dir.as_deref())
    }
}

/// 解析 `KEY=VALUE` 多行文本，忽略空行与 `#` 注释行。
///
/// 环境变量名必须只含字母、数字与下划线，避免 `:` 等字符破坏 `WSLENV` 的拼接格式。
pub(crate) fn parse_env(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("第 {} 行缺少 '='：{line}", index + 1));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("第 {} 行环境变量名不能为空", index + 1));
        }
        let valid = key.chars().enumerate().all(|(position, ch)| {
            ch == '_' || ch.is_ascii_alphabetic() || (position > 0 && ch.is_ascii_digit())
        });
        if !valid {
            return Err(format!(
                "第 {} 行环境变量名 {key} 不合法（只允许字母、数字与下划线，且不能以数字开头）",
                index + 1
            ));
        }
        entries.push((key.to_string(), value.trim().to_string()));
    }
    Ok(entries)
}

/// 构造 WSL 的配置 Schema。
///
/// `distributions` 来自 `list_distributions()`：能枚举时用 `Select` 提供候选，
/// 枚举失败时退化为 `Text`；无论哪种，`distribution` 都允许填写任意发行版名
/// （Schema 由启动时枚举结果生成，可能落后于用户实际安装情况）。
pub(crate) fn schema(distributions: &[String]) -> ConfigSchema {
    let kind = if distributions.is_empty() {
        FieldKind::Text
    } else {
        FieldKind::Select {
            options: distributions
                .iter()
                .map(|name| SelectOption {
                    value: name.clone(),
                    label: name.clone(),
                })
                .collect(),
        }
    };

    let distribution = ConfigField {
        placeholder: if distributions.is_empty() {
            Some("Ubuntu".to_string())
        } else {
            None
        },
        help: Some("WSL 发行版名称，也可填写未列出的已安装发行版".to_string()),
        validation: Some(FieldValidation {
            min_length: Some(1),
            ..Default::default()
        }),
        ..field("distribution", "发行版", kind, true)
    };

    let working_dir = ConfigField {
        placeholder: Some("/home/user".to_string()),
        help: Some("会话默认工作目录，通过 wsl.exe --cd 传入".to_string()),
        ..field("working_dir", "默认工作目录", FieldKind::Text, false)
    };

    let shell = ConfigField {
        default_value: Some(DEFAULT_SHELL.into()),
        help: Some("发行版内的 shell 绝对路径".to_string()),
        validation: Some(FieldValidation {
            min_length: Some(1),
            ..Default::default()
        }),
        ..field("shell", "Shell", FieldKind::Text, false)
    };

    let startup_args = ConfigField {
        placeholder: Some("-lc".to_string()),
        help: Some(
            "exec 与连接测试时的 shell 启动参数，按空白拆分；交互式终端不追加该参数".to_string(),
        ),
        ..field("startup_args", "启动参数", FieldKind::Text, false)
    };

    let timeout = ConfigField {
        default_value: Some(DEFAULT_TIMEOUT_SECS.into()),
        help: Some("单条命令/连接测试的最长执行时间，超时会结束子进程".to_string()),
        validation: Some(FieldValidation {
            min: Some(MIN_TIMEOUT_SECS),
            max: Some(MAX_TIMEOUT_SECS),
            ..Default::default()
        }),
        ..field(
            "process_timeout_secs",
            "进程超时（秒）",
            FieldKind::Number,
            false,
        )
    };

    let env = ConfigField {
        placeholder: Some("LANG=C.UTF-8\nTERM=xterm-256color".to_string()),
        help: Some("每行一个 KEY=VALUE，通过 WSLENV 传入发行版".to_string()),
        ..field("env", "环境变量", FieldKind::Multiline, false)
    };

    ConfigSchema::new(
        "wsl",
        vec![distribution, working_dir, shell, startup_args, timeout, env],
    )
}

fn field(key: &str, label: &str, kind: FieldKind, required: bool) -> ConfigField {
    ConfigField {
        key: key.to_string(),
        label: label.to_string(),
        kind,
        required,
        default_value: None,
        placeholder: None,
        help: None,
        secret: false,
        visible_when: None,
        validation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_lines() {
        let env = parse_env("A=1\r\nB=two=three\n\n# 注释\n C = 3 \n").expect("应能解析");
        assert_eq!(
            env,
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "two=three".to_string()),
                ("C".to_string(), "3".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_env_without_equals() {
        let error = parse_env("A=1\nNOVALUE\n").unwrap_err();
        assert!(error.contains("第 2 行"), "错误应带行号：{error}");
    }

    #[test]
    fn rejects_env_with_illegal_name() {
        let error = parse_env("1BAD=x\n").unwrap_err();
        assert!(error.contains("不合法"), "错误应说明变量名非法：{error}");
        let error = parse_env("A:B=x\n").unwrap_err();
        assert!(error.contains("不合法"), "冒号会破坏 WSLENV：{error}");
    }

    #[test]
    fn applies_defaults() {
        let config = crate::test_config(r#"{"distribution":"Ubuntu"}"#);
        let parsed = WslConfig::from_config(&config).expect("应能解析");
        assert_eq!(parsed.shell, DEFAULT_SHELL);
        assert_eq!(parsed.timeout, Duration::from_secs(30));
        assert!(parsed.env.is_empty());
        assert_eq!(parsed.exec_shell_args(), vec!["-lc".to_string()]);
        assert_eq!(
            parsed.exec_args("uname -a", None),
            vec![
                "-d".to_string(),
                "Ubuntu".to_string(),
                "--".to_string(),
                DEFAULT_SHELL.to_string(),
                "-lc".to_string(),
                "uname -a".to_string(),
            ]
        );
    }

    #[test]
    fn builds_exec_args_with_working_dir_and_startup_args() {
        let config = crate::test_config(
            r#"{"distribution":"Ubuntu","working_dir":"/srv","shell":"/bin/zsh","startup_args":"-l -c"}"#,
        );
        let parsed = WslConfig::from_config(&config).expect("应能解析");
        assert_eq!(parsed.exec_shell_args(), vec!["-l", "-c"]);
        assert_eq!(
            parsed.exec_args("pwd", parsed.working_dir.as_deref()),
            vec![
                "-d".to_string(),
                "Ubuntu".to_string(),
                "--cd".to_string(),
                "/srv".to_string(),
                "--".to_string(),
                "/bin/zsh".to_string(),
                "-l".to_string(),
                "-c".to_string(),
                "pwd".to_string(),
            ]
        );
        assert_eq!(
            parsed.interactive_args(),
            vec![
                "-d".to_string(),
                "Ubuntu".to_string(),
                "--cd".to_string(),
                "/srv".to_string(),
                "--".to_string(),
                "/bin/zsh".to_string(),
            ]
        );
    }

    #[test]
    fn env_override_replaces_existing_key() {
        let config = crate::test_config(r#"{"distribution":"Ubuntu","env":"A=1\nB=2"}"#);
        let mut parsed = WslConfig::from_config(&config).expect("应能解析");
        parsed.set_env("A", "9");
        parsed.set_env("C", "3");
        assert_eq!(
            parsed.env,
            vec![
                ("A".to_string(), "9".to_string()),
                ("B".to_string(), "2".to_string()),
                ("C".to_string(), "3".to_string()),
            ]
        );
    }
}
