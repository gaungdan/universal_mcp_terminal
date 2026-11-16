// 首页快速开始所需的提示词生成工具。

/**
 * 生成 AI 接入提示词：直接包含所选 Token 的 MCP 配置，
 * 其余交给 AI 自行探索，只保留必要的行为约束。
 */
export function buildAiPrompt(config: string): string {
  return [
    "请将下面的 MCP 配置加入你的 MCP 客户端并重新加载，然后自行调用 tools/list 探索可用工具：",
    "",
    config,
    "",
    "只通过该 MCP Server 操作已授权连接；高风险操作先向用户确认，不要虚构执行结果。",
  ].join("\n");
}
