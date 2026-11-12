//! WSL Provider：通过 `wsl.exe` 管理发行版子进程，使用 Job Object 保证进程不泄漏。
//!
//! 独占写入范围：`crates/providers/wsl/**`。

#![deny(unsafe_code)]
