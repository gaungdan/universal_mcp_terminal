//! 协议 Provider 统一抽象：`ProtocolProvider`、`TerminalSession`、`ConfigSchema`、`ConnectionTestResult`。
//!
//! 约束：本 crate 不允许出现任何具体协议（SSH/Telnet/WSL）的分支逻辑。
//! 契约冻结后由 T2 维护，其他工单只读。

#![deny(unsafe_code)]
