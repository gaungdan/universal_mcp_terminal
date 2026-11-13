//! WSL Provider：通过 `wsl.exe` 管理发行版子进程，使用 Job Object 保证进程不泄漏。
//!
//! 独占写入范围：`crates/providers/wsl/**`。
//!
//! # unsafe 策略
//!
//! 会话退出时必须结束整棵子进程树（`wsl.exe` 及其托管的发行版进程），否则会进程泄漏。
//! Windows 上可靠的做法是把子进程放进设置了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
//! 的 Job Object，而 `windows-sys` 只提供 unsafe FFI。因此：
//!
//! - 非 Windows 平台：crate 级 `deny(unsafe_code)`，完全禁止 unsafe；
//! - Windows：只允许 `job` 模块的 Windows 实现使用 unsafe（模块上 `#[allow(unsafe_code)]`），
//!   且每个 unsafe 块都用 SAFETY 注释说明句柄来源与有效期。
//!
//! 除 Job Object 外，本 crate 的其余代码不使用任何 unsafe。

#![cfg_attr(not(windows), deny(unsafe_code))]

/// 构造测试用 `ConnectionConfig`。
///
/// 本 crate 不依赖 `serde_json`（Cargo.toml 由主控维护、不可修改），因此不能写 `json!`；
/// 改为把 JSON 文本解析成 `ConnectionConfig::new` 要求的 `serde_json::Value`
/// （`Value: FromStr`，解析目标类型由函数参数推断）。
#[cfg(test)]
pub(crate) fn test_config(json: &str) -> umt_core::ConnectionConfig {
    umt_core::ConnectionConfig::new("wsl", json.parse().expect("测试配置必须是合法 JSON 对象"))
}

pub mod provider;
pub mod session;

mod config;
mod decode;
mod job;
mod process;

pub use provider::{WslProvider, list_distributions};
pub use session::WslSession;
