//! 应用服务层：把各 crate 组装成可直接被 IPC 命令调用的服务。
//!
//! 命令层只做参数透传与 `ApiError` 转换，业务逻辑集中在这里，便于集成测试。

pub mod audit;
pub mod connections;
pub mod mcp;
pub mod protocols;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod state;
pub mod store;
pub mod tokens;

use std::fmt::Display;
use std::str::FromStr;
use umt_core::UmtError;

/// 解析 IPC 传入的强类型 ID 字符串。
pub(crate) fn parse_id<T>(value: &str, entity: &'static str) -> Result<T, UmtError>
where
    T: FromStr,
    T::Err: Display,
{
    value
        .parse::<T>()
        .map_err(|_| UmtError::Validation(format!("{entity} ID 格式非法：{value}")))
}
