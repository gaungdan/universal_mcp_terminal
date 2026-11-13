// 首页快速开始所需的配置与提示词生成工具（均不含真实 Token 之外的敏感信息）。

export const MCP_SERVER_NAME = "universal-mcp-terminal";
export const MCP_TRANSPORT = "streamable-http";

export function buildMcpConfig(endpoint: string, token: string): string {
  return JSON.stringify(
    {
      mcpServers: {
        [MCP_SERVER_NAME]: {
          transport: MCP_TRANSPORT,
          url: endpoint,
          headers: {
            Authorization: `Bearer ${token}`,
          },
        },
      },
    },
    null,
    2,
  );
}

export function buildMaskedMcpConfig(endpoint: string): string {
  return buildMcpConfig(endpoint, "••••••••••••");
}

/**
 * 按需求 §8 Step 5 生成 AI 接入提示词。
 * 只包含服务地址与规则，不包含任何 Token 明文。
 */
export function buildAiPrompt(endpoint: string): string {
  return [
    "你现在可以通过 Universal MCP Terminal MCP Server 管理本地授权的远程连接。",
    "",
    "MCP 服务地址：",
    "",
    endpoint,
    "",
    "Transport：Streamable HTTP。",
    "",
    "Authentication：Bearer Token。",
    "",
    "请遵守以下规则：",
    "",
    "1. 只能使用 Universal MCP Terminal 提供的 MCP Tools 操作设备。",
    "2. 不要自行编写 SSH、Telnet 或 WSL 连接脚本。",
    "3. 不要绕过 Universal MCP Terminal 的访问控制。",
    "4. 只能操作当前 Token 授权的连接。",
    "5. 操作前先调用 list_connections，了解可用连接。",
    "6. 执行命令前说明准备执行的操作。",
    "7. 涉及删除文件、修改配置、重启服务、修改网络等高风险操作，必须先向用户确认。",
    "8. 不要尝试读取、导出或泄露连接密码、私钥及 Token。",
    "9. 所有设备操作必须通过 MCP 完成。",
    "10. 发生连接失败或执行异常时，返回真实错误信息，不要虚构执行结果。",
    "",
    "请先调用 list_connections，查看当前 Token 可以访问的连接。",
  ].join("\n");
}

export function maskTokenPrefix(prefix: string): string {
  return `umt_${prefix}_••••••`;
}
