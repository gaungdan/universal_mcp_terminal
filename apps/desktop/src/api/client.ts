// 与 `crates/app/src/ipc/commands.rs` 一一对应的调用封装（T5 契约）。
// 所有命令返回 Result<T, ApiError>，Tauri 会把 Err 作为 rejected promise 抛出。

import { invoke } from "@tauri-apps/api/core";
import type {
  AuditDto,
  AuditQueryDto,
  CloseSessionInput,
  ConnectionDetailDto,
  ConnectionDto,
  ConnectionFilterDto,
  ConnectionTestResult,
  CreateConnectionInput,
  CreateTokenInput,
  CreatedTokenDto,
  DraftConnectionTestInput,
  ExportResultDto,
  GrantDto,
  HostKeyDto,
  McpConfigInput,
  McpStatusDto,
  OverviewDto,
  PageDto,
  PageRequestDto,
  ProtocolInfoDto,
  SessionDto,
  SessionFilterDto,
  SetGrantInput,
  SetTokenStatusInput,
  SettingsDto,
  TokenDto,
  TrustHostKeyInput,
  UpdateConnectionInput,
  UpdateSettingsInput,
  UpdateTokenInput,
} from "./types";

export function getOverview(): Promise<OverviewDto> {
  return invoke<OverviewDto>("get_overview");
}

export function listProtocols(): Promise<ProtocolInfoDto[]> {
  return invoke<ProtocolInfoDto[]>("list_protocols");
}

// ---------------- 连接 ----------------

export function listConnections(filter: ConnectionFilterDto): Promise<PageDto<ConnectionDto>> {
  return invoke<PageDto<ConnectionDto>>("list_connections", { filter });
}

export function getConnection(id: string): Promise<ConnectionDetailDto> {
  return invoke<ConnectionDetailDto>("get_connection", { id });
}

export function createConnection(input: CreateConnectionInput): Promise<ConnectionDto> {
  return invoke<ConnectionDto>("create_connection", { input });
}

export function updateConnection(input: UpdateConnectionInput): Promise<ConnectionDto> {
  return invoke<ConnectionDto>("update_connection", { input });
}

export function deleteConnection(id: string): Promise<boolean> {
  return invoke<boolean>("delete_connection", { id });
}

export function setConnectionEnabled(id: string, enabled: boolean): Promise<ConnectionDto> {
  return invoke<ConnectionDto>("set_connection_enabled", { id, enabled });
}

export function duplicateConnection(id: string): Promise<ConnectionDto> {
  return invoke<ConnectionDto>("duplicate_connection", { id });
}

export function testConnection(id: string): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("test_connection", { id });
}

export function testConnectionDraft(
  input: DraftConnectionTestInput,
): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("test_connection_draft", { input });
}

// ---------------- SSH Host Key 信任 ----------------

export function listHostKeys(): Promise<HostKeyDto[]> {
  return invoke<HostKeyDto[]>("list_host_keys");
}

export function trustHostKey(input: TrustHostKeyInput): Promise<HostKeyDto> {
  return invoke<HostKeyDto>("trust_host_key", { input });
}

export function deleteHostKey(endpoint: string): Promise<boolean> {
  return invoke<boolean>("delete_host_key", { endpoint });
}

// ---------------- 访问控制 ----------------

export function listTokens(page: PageRequestDto): Promise<PageDto<TokenDto>> {
  return invoke<PageDto<TokenDto>>("list_tokens", { page });
}

export function createToken(input: CreateTokenInput): Promise<CreatedTokenDto> {
  return invoke<CreatedTokenDto>("create_token", { input });
}

export function getTokenMcpConfig(tokenId: string): Promise<string> {
  return invoke<string>("get_token_mcp_config", { tokenId });
}

export function updateToken(input: UpdateTokenInput): Promise<TokenDto> {
  return invoke<TokenDto>("update_token", { input });
}

export function setTokenStatus(input: SetTokenStatusInput): Promise<TokenDto> {
  return invoke<TokenDto>("set_token_status", { input });
}

export function deleteToken(id: string): Promise<boolean> {
  return invoke<boolean>("delete_token", { id });
}

export function listTokenGrants(tokenId: string): Promise<GrantDto[]> {
  return invoke<GrantDto[]>("list_token_grants", { tokenId });
}

export function setTokenGrant(input: SetGrantInput): Promise<GrantDto> {
  return invoke<GrantDto>("set_token_grant", { input });
}

// ---------------- 会话 ----------------

export function listActiveSessions(): Promise<SessionDto[]> {
  return invoke<SessionDto[]>("list_active_sessions");
}

export function listSessionHistory(filter: SessionFilterDto): Promise<PageDto<SessionDto>> {
  return invoke<PageDto<SessionDto>>("list_session_history", { filter });
}

export function closeSession(input: CloseSessionInput): Promise<boolean> {
  return invoke<boolean>("close_session", { input });
}

// ---------------- 审计 ----------------

export function queryAudit(query: AuditQueryDto): Promise<PageDto<AuditDto>> {
  return invoke<PageDto<AuditDto>>("query_audit", { query });
}

export function getAudit(id: number): Promise<AuditDto> {
  return invoke<AuditDto>("get_audit", { id });
}

export function exportAudit(query: AuditQueryDto): Promise<ExportResultDto> {
  return invoke<ExportResultDto>("export_audit", { query });
}

// ---------------- MCP 服务 ----------------

export function getMcpStatus(): Promise<McpStatusDto> {
  return invoke<McpStatusDto>("get_mcp_status");
}

export function startMcp(config?: McpConfigInput): Promise<McpStatusDto> {
  return invoke<McpStatusDto>("start_mcp", { config: config ?? null });
}

export function stopMcp(): Promise<McpStatusDto> {
  return invoke<McpStatusDto>("stop_mcp");
}

export function updateMcpSettings(input: McpConfigInput): Promise<McpStatusDto> {
  return invoke<McpStatusDto>("update_mcp_settings", { input });
}

// ---------------- 系统设置 ----------------

export function getSettings(): Promise<SettingsDto> {
  return invoke<SettingsDto>("get_settings");
}

export function updateSettings(input: UpdateSettingsInput): Promise<SettingsDto> {
  return invoke<SettingsDto>("update_settings", { input });
}
