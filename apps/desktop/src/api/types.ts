// 本文件与 Rust 侧 `crates/app/src/ipc/dto.rs` 逐字段对齐（T5 契约，冻结后只读）。
// 修改任一侧都必须同步另一侧，并在 PR 中说明。

export type ApiError = {
  code: string;
  message: string;
};

export type PageDto<T> = {
  items: T[];
  total: number;
};

export type PageRequestDto = {
  offset: number;
  limit: number;
};

// ---------------- 协议配置 Schema ----------------

export type SelectOption = {
  value: string;
  label: string;
};

export type FieldKind =
  | { kind: "text" }
  | { kind: "password" }
  | { kind: "number" }
  | { kind: "boolean" }
  | { kind: "select"; options: SelectOption[] }
  | { kind: "multiline" }
  | { kind: "path" };

export type FieldValidation = {
  minLength?: number;
  maxLength?: number;
  min?: number;
  max?: number;
  pattern?: string;
};

export type FieldCondition = {
  key: string;
  equals: unknown;
};

export type ConfigField = {
  key: string;
  label: string;
  kind: FieldKind;
  required: boolean;
  defaultValue?: unknown;
  placeholder?: string;
  help?: string;
  secret: boolean;
  visibleWhen?: FieldCondition;
  validation?: FieldValidation;
};

export type ConfigSchema = {
  protocolId: string;
  fields: ConfigField[];
};

export type ProtocolInfoDto = {
  protocolId: string;
  displayName: string;
  version: string;
  description: string;
  capabilities: string[];
  configSchema: ConfigSchema;
};

// ---------------- 连接 ----------------

export type ConnectionDto = {
  id: string;
  name: string;
  protocolId: string;
  protocolDisplayName: string;
  description?: string | null;
  tags: string[];
  notes?: string | null;
  enabled: boolean;
  capabilities: string[];
  hasSecret: boolean;
  createdAt: number;
  updatedAt: number;
  revision: number;
};

export type ConnectionDetailDto = {
  connection: ConnectionDto;
  config: Record<string, unknown>;
  secretFieldsSet: string[];
};

export type SecretInput = {
  key: string;
  value: string;
};

export type CreateConnectionInput = {
  name: string;
  protocolId: string;
  description?: string | null;
  tags: string[];
  notes?: string | null;
  enabled: boolean;
  config: Record<string, unknown>;
  secrets: SecretInput[];
};

export type UpdateConnectionInput = {
  id: string;
  name: string;
  description?: string | null;
  tags: string[];
  notes?: string | null;
  enabled: boolean;
  config: Record<string, unknown>;
  secrets?: SecretInput[] | null;
  expectedRevision: number;
};

export type ConnectionFilterDto = PageRequestDto & {
  protocolId?: string | null;
  enabled?: boolean | null;
  search?: string | null;
};

export type DraftConnectionTestInput = {
  protocolId: string;
  config: Record<string, unknown>;
  secrets: SecretInput[];
};

export type TestStepStatus = "passed" | "failed" | "skipped";

export type TestStep = {
  name: string;
  status: TestStepStatus;
  message?: string | null;
  durationMs?: number | null;
};

export type ConnectionTestResult = {
  ok: boolean;
  message: string;
  latencyMs?: number | null;
  steps: TestStep[];
  suggestion?: string | null;
  /** 握手阶段观察到的 Host Key，可用于引导用户信任。 */
  hostKey?: HostKeyInfo | null;
};

export type HostKeyInfo = {
  fingerprint: string;
  algorithm?: string | null;
};

export type HostKeyDto = {
  endpoint: string;
  host: string;
  port: number;
  algorithm?: string | null;
  fingerprint: string;
  trustedAt: number;
  trustedBy?: string | null;
};

export type TrustHostKeyInput = {
  connectionId: string;
  fingerprint: string;
  algorithm?: string | null;
};

// ---------------- Token 与授权 ----------------

export type TokenStatus = "active" | "disabled" | "revoked";

export type TokenDto = {
  id: string;
  name: string;
  description?: string | null;
  prefix: string;
  status: TokenStatus;
  expiresAt?: number | null;
  createdAt: number;
  lastUsedAt?: number | null;
};

export type CreateTokenInput = {
  name: string;
  description?: string | null;
  expiresAt?: number | null;
};

export type CreatedTokenDto = {
  token: TokenDto;
  plaintext: string;
};

export type UpdateTokenInput = {
  id: string;
  name: string;
  description?: string | null;
  expiresAt?: number | null;
};

export type SetTokenStatusInput = {
  id: string;
  status: TokenStatus;
};

export type GrantDto = {
  connection: ConnectionDto;
  allowed: boolean;
  grantedAt?: number | null;
};

export type SetGrantInput = {
  tokenId: string;
  connectionId: string;
  allowed: boolean;
};

// ---------------- 会话 ----------------

export type SessionState =
  | "connecting"
  | "connected"
  | "running"
  | "closed"
  | "failed"
  | "interrupted";

export type SessionDto = {
  id: string;
  tokenId: string;
  connectionId: string;
  protocolId: string;
  clientInfo?: string | null;
  state: SessionState;
  startedAt: number;
  endedAt?: number | null;
  closeReason?: string | null;
  rows: number;
  cols: number;
};

export type SessionFilterDto = PageRequestDto & {
  tokenId?: string | null;
  connectionId?: string | null;
};

export type CloseSessionInput = {
  sessionId: string;
  reason?: string | null;
};

// ---------------- 审计 ----------------

export type AuditKind =
  | "session_opened"
  | "session_closed"
  | "command_executed"
  | "input_sent"
  | "output_read"
  | "permission_denied"
  | "connection_created"
  | "connection_updated"
  | "connection_deleted"
  | "connection_tested"
  | "token_created"
  | "token_disabled"
  | "token_revoked"
  | "grant_changed"
  | "mcp_server_started"
  | "mcp_server_stopped";

export type AuditOutcome = "allowed" | "denied" | "failed";

export type ExecutionState =
  | "sent"
  | "running"
  | "succeeded"
  | "failed"
  | "timed_out"
  | "unknown";

export type AuditDto = {
  id: number;
  sessionId?: string | null;
  tokenId?: string | null;
  connectionId?: string | null;
  kind: AuditKind;
  outcome: AuditOutcome;
  command?: string | null;
  workingDir?: string | null;
  startedAt: number;
  durationMs?: number | null;
  state: ExecutionState;
  exitCode?: number | null;
  stdoutExcerpt?: string | null;
  stderrExcerpt?: string | null;
  error?: string | null;
  redacted: boolean;
};

export type AuditQueryDto = PageRequestDto & {
  tokenId?: string | null;
  connectionId?: string | null;
  sessionId?: string | null;
  kind?: AuditKind | null;
  outcome?: AuditOutcome | null;
  from?: number | null;
  to?: number | null;
};

export type ExportResultDto = {
  path: string;
  rows: number;
};

// ---------------- MCP 与设置 ----------------

export type McpStatusDto = {
  running: boolean;
  host: string;
  port: number;
  endpoint: string;
  lastError?: string | null;
};

export type McpConfigInput = {
  host: string;
  port: number;
};

export type SecuritySettingsDto = {
  defaultExecutionMode: string;
  sessionTimeoutSecs: number;
  requireConfirmationForHighRisk: boolean;
};

export type SettingsDto = {
  mcp: McpStatusDto;
  security: SecuritySettingsDto;
  logLevel: string;
};

export type UpdateSettingsInput = {
  security: SecuritySettingsDto;
  logLevel: string;
};

export type OverviewDto = {
  connectionCount: number;
  enabledConnectionCount: number;
  tokenCount: number;
  activeSessionCount: number;
  mcp: McpStatusDto;
  recentAudits: AuditDto[];
};

// ---------------- 后端推送事件 ----------------

export type AppEvent =
  | { type: "connection_changed"; connectionId: string }
  | { type: "token_changed"; tokenId: string }
  | { type: "grant_changed"; tokenId: string; connectionId: string; allowed: boolean }
  | { type: "session_changed"; sessionId: string; state: SessionState }
  | { type: "mcp_server_state_changed"; running: boolean; endpoint?: string | null }
  | { type: "audit_appended"; auditId: number };
