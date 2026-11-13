// 首页快速开始所需的提示词生成工具。

/**
 * 生成 AI 接入提示词：直接包含所选 Token 的 MCP 配置，
 * 只说明接入方式与安全约定，具体能力由 AI 接入后自行探索。
 */
export function buildAiPrompt(config: string): string {
  return [
    "请按以下说明接入 Universal MCP Terminal 提供的 MCP Server。",
    "",
    "第 1 步：把下面的 MCP 配置添加到你的 MCP 客户端配置文件中：",
    "",
    config,
    "",
    "第 2 步：保存配置后重启或重新加载 MCP 客户端，确认 universal-mcp-terminal 连接成功。该服务使用 Streamable HTTP 传输，通过 Authorization: Bearer Token 鉴权。",
    "",
    "第 3 步：连接成功后，先调用工具列表（tools/list）自行探索该 Server 实际提供的工具、参数和限制，再根据用户需求选择并调用合适的工具。不要假设工具能力，也不要虚构执行结果。",
    "",
    "接入后的行为约定：",
    "",
    "1. 只通过 Universal MCP Terminal 提供的 MCP 工具操作远程连接，不要自行编写 SSH、Telnet 或 WSL 连接脚本。",
    "2. 不要绕过 Universal MCP Terminal 的访问控制，只能操作当前 Token 已授权的连接。",
    "3. 执行命令前先说明准备执行的操作。",
    "4. 涉及删除文件、修改配置、重启服务、修改网络等高风险操作，必须先向用户确认。",
    "5. 不要尝试读取、导出或泄露连接密码、私钥及 Token。",
    "6. 发生连接失败或执行异常时，返回真实错误信息，不要虚构执行结果。",
  ].join("\n");
}

export function maskTokenPrefix(prefix: string): string {
  return `umt_${prefix}_••••••`;
}
