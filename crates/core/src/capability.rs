use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 协议能力声明。Provider 只允许声明自己真实支持的能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// 交互式 Shell。
    Shell,
    /// 一次性非交互命令执行。
    Exec,
    /// 可保持的交互式终端。
    InteractiveTerminal,
    /// 读取设备/环境信息。
    DeviceInformation,
    /// 端口转发。
    PortForward,
    /// 终端尺寸调整。
    TerminalResize,
    /// 读取环境变量等环境信息。
    EnvironmentInfo,
    /// 只读文件内容（SFTP 等）。
    FileRead,
}

impl Capability {
    /// 稳定字符串标识，用于日志、审计与前端展示，不随 serde 改名而变化。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Exec => "exec",
            Self::InteractiveTerminal => "interactive_terminal",
            Self::DeviceInformation => "device_information",
            Self::PortForward => "port_forward",
            Self::TerminalResize => "terminal_resize",
            Self::EnvironmentInfo => "environment_info",
            Self::FileRead => "file_read",
        }
    }
}

/// 能力集合。序列化为字符串数组，便于 IPC 与 MCP 工具共用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    pub fn of(items: impl IntoIterator<Item = Capability>) -> Self {
        Self(items.into_iter().collect())
    }

    pub fn insert(&mut self, capability: Capability) {
        self.0.insert(capability);
    }

    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    pub fn as_strings(&self) -> Vec<&'static str> {
        self.0.iter().map(|c| c.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self::of(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_only_declared_capabilities() {
        let set = CapabilitySet::of([Capability::Shell, Capability::Exec]);
        assert!(set.contains(Capability::Shell));
        assert!(!set.contains(Capability::PortForward));
    }

    #[test]
    fn serializes_as_string_array() {
        let set = CapabilitySet::of([Capability::Shell, Capability::TerminalResize]);
        let json = serde_json::to_value(&set).expect("序列化失败");
        assert_eq!(json, serde_json::json!(["shell", "terminal_resize"]));
    }
}
