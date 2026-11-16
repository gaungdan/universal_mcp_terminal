//! 10 个 MCP 工具的定义与入参结构。
//!
//! 这里只描述「工具长什么样」，不包含任何权限判断；鉴权与能力校验在 `handler` 中完成。
//! 入参 JSON Schema 由 `schemars` 根据参数结构生成，避免手写 Schema 与代码脱节。

use rmcp::model::{JsonObject, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// 列出当前 Token 已授权的连接。
pub const LIST_CONNECTIONS: &str = "list_connections";
/// 获取单个连接的详细信息（不含凭据值）。
pub const GET_CONNECTION_INFO: &str = "get_connection_info";
/// 对指定连接做真实连接测试。
pub const TEST_CONNECTION: &str = "test_connection";
/// 打开展交互式会话。
pub const OPEN_SESSION: &str = "open_session";
/// 执行非交互式命令。
pub const EXECUTE_COMMAND: &str = "execute_command";
/// 只读读取远程文件分块。
pub const READ_FILE: &str = "read_file";
/// 向已有会话发送输入。
pub const SEND_INPUT: &str = "send_input";
/// 增量读取会话输出。
pub const READ_OUTPUT: &str = "read_output";
/// 调整终端尺寸。
pub const RESIZE_TERMINAL: &str = "resize_terminal";
/// 关闭会话。
pub const CLOSE_SESSION: &str = "close_session";

/// 全部工具名，顺序与 `tool_definitions` 一致。
pub const TOOL_NAMES: [&str; 10] = [
    LIST_CONNECTIONS,
    GET_CONNECTION_INFO,
    TEST_CONNECTION,
    OPEN_SESSION,
    EXECUTE_COMMAND,
    READ_FILE,
    SEND_INPUT,
    READ_OUTPUT,
    RESIZE_TERMINAL,
    CLOSE_SESSION,
];

/// `list_connections` 无参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListConnectionsArgs {}

/// 只带连接 ID 的工具参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConnectionArgs {
    /// 连接 ID。
    pub connection_id: String,
}

/// `open_session` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenSessionArgs {
    /// 连接 ID。
    pub connection_id: String,
    /// 终端行数。
    pub rows: u16,
    /// 终端列数。
    pub cols: u16,
    /// 终端类型，例如 `xterm-256color`。
    pub terminal_type: Option<String>,
}

/// `execute_command` 参数。`connection_id` 与 `session_id` 二选一。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteCommandArgs {
    /// 连接 ID；提供时会自动创建临时会话并在结束后关闭。
    pub connection_id: Option<String>,
    /// 已打开的会话 ID；与 `connection_id` 互斥。
    pub session_id: Option<String>,
    /// 要执行的命令。
    pub command: String,
    /// 工作目录。
    pub working_directory: Option<String>,
    /// 超时时间（毫秒），默认 30000。
    pub timeout_ms: Option<u64>,
}

/// `read_file` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    /// 连接 ID。
    pub connection_id: String,
    /// 远程文件绝对路径。
    pub path: String,
    /// 起始字节偏移，默认 0。
    pub offset: Option<u64>,
    /// 单次读取字节数，默认 65536，上限 131072。
    pub max_bytes: Option<usize>,
}

/// `send_input` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendInputArgs {
    /// 会话 ID。
    pub session_id: String,
    /// 原始输入内容（交互式终端，可包含换行）。
    pub input: String,
}

/// `read_output` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadOutputArgs {
    /// 会话 ID。
    pub session_id: String,
    /// 上次读取返回的 cursor；首次读取传 0。
    pub cursor: u64,
    /// 单次最多读取的字节数，默认 65536。
    pub max_bytes: Option<usize>,
}

/// `resize_terminal` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResizeTerminalArgs {
    /// 会话 ID。
    pub session_id: String,
    /// 终端行数。
    pub rows: u16,
    /// 终端列数。
    pub cols: u16,
}

/// `close_session` 参数。
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseSessionArgs {
    /// 会话 ID。
    pub session_id: String,
}

/// 构造全部工具定义。
pub fn tool_definitions() -> Vec<Tool> {
    vec![
        definition::<ListConnectionsArgs>(
            LIST_CONNECTIONS,
            "列出当前 Token 被授权的连接（仅 id、名称、协议、描述、能力与启用状态，不含任何凭据）。",
        ),
        definition::<ConnectionArgs>(
            GET_CONNECTION_INFO,
            "获取当前 Token 有权访问的连接信息，并列出已配置的敏感字段名（不返回值）。",
        ),
        definition::<ConnectionArgs>(
            TEST_CONNECTION,
            "对指定连接执行真实握手/认证测试；调用前会校验 Token 授权。",
        ),
        definition::<OpenSessionArgs>(OPEN_SESSION, "在指定连接上打开终端会话，返回 session_id。"),
        definition::<ExecuteCommandArgs>(
            EXECUTE_COMMAND,
            "执行非交互式命令。传 connection_id 时自动开临时会话并在结束后关闭；协议必须声明 exec 能力。",
        ),
        definition::<ReadFileArgs>(
            READ_FILE,
            "只读读取远程文件分块，返回 base64 数据、下一偏移与 eof；协议必须声明 file_read 能力。",
        ),
        definition::<SendInputArgs>(SEND_INPUT, "向已有交互式会话发送原始输入。"),
        definition::<ReadOutputArgs>(
            READ_OUTPUT,
            "按 cursor 增量读取会话输出，返回文本、新 cursor 与 eof。",
        ),
        definition::<ResizeTerminalArgs>(
            RESIZE_TERMINAL,
            "调整终端尺寸；协议未声明 terminal_resize 能力时返回错误。",
        ),
        definition::<CloseSessionArgs>(CLOSE_SESSION, "关闭会话并释放资源。"),
    ]
}

fn definition<A: JsonSchema>(name: &'static str, description: &'static str) -> Tool {
    Tool::new(name, description, input_schema::<A>())
}

fn input_schema<A: JsonSchema>() -> JsonObject {
    let value = serde_json::to_value(schemars::schema_for!(A)).expect("入参 Schema 序列化失败");
    let Value::Object(mut object) = value else {
        panic!("MCP 工具入参 Schema 必须是 JSON 对象");
    };
    // 顶层 title/description 来自类型名，对 LLM 是噪音；根类型固定为 object。
    object.remove("title");
    object.remove("description");
    object.insert("type".into(), Value::String("object".into()));
    object
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_cover_all_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), TOOL_NAMES.len());

        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(names, TOOL_NAMES.to_vec());

        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "工具名必须唯一");
    }

    #[test]
    fn every_tool_takes_an_object_schema() {
        for tool in tool_definitions() {
            assert_eq!(
                tool.input_schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{} 的入参 Schema 根类型必须是 object",
                tool.name
            );
            assert!(tool.description.is_some(), "{} 缺少描述", tool.name);
        }
    }

    #[test]
    fn open_session_schema_declares_expected_fields() {
        let tools = tool_definitions();
        let tool = tools
            .iter()
            .find(|tool| tool.name == OPEN_SESSION)
            .expect("缺少 open_session");
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("缺少 properties");
        for field in ["connection_id", "rows", "cols", "terminal_type"] {
            assert!(properties.contains_key(field), "缺少字段 {field}");
        }
    }
}
