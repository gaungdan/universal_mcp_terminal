import type {
  AuditKind,
  AuditOutcome,
  AuditQueryDto,
  ExecutionState,
} from "../../api/types";
import { isApiError, toUserMessage } from "../../lib/errors";

export const AUDIT_PAGE_SIZE = 50;
export const EXPORT_LIMIT = 10_000;

export type AuditFilterDraft = {
  from: string;
  to: string;
  tokenId: string;
  connectionId: string;
  sessionId: string;
  kind: "" | AuditKind;
  outcome: "" | AuditOutcome;
  state: "" | ExecutionState;
};

export const EMPTY_AUDIT_FILTER: AuditFilterDraft = {
  from: "",
  to: "",
  tokenId: "",
  connectionId: "",
  sessionId: "",
  kind: "",
  outcome: "",
  state: "",
};

export const AUDIT_KIND_OPTIONS: ReadonlyArray<{ value: AuditKind; label: string }> = [
  { value: "session_opened", label: "会话开启" },
  { value: "session_closed", label: "会话关闭" },
  { value: "command_executed", label: "命令执行" },
  { value: "file_read", label: "文件读取" },
  { value: "input_sent", label: "输入发送" },
  { value: "output_read", label: "输出读取" },
  { value: "permission_denied", label: "权限拒绝" },
  { value: "connection_created", label: "连接创建" },
  { value: "connection_updated", label: "连接更新" },
  { value: "connection_deleted", label: "连接删除" },
  { value: "connection_tested", label: "连接测试" },
  { value: "token_created", label: "Token 创建" },
  { value: "token_disabled", label: "Token 禁用" },
  { value: "token_revoked", label: "Token 撤销" },
  { value: "grant_changed", label: "授权变更" },
  { value: "mcp_server_started", label: "MCP 服务启动" },
  { value: "mcp_server_stopped", label: "MCP 服务停止" },
];

export const AUDIT_OUTCOME_OPTIONS: ReadonlyArray<{
  value: AuditOutcome;
  label: string;
}> = [
  { value: "allowed", label: "允许" },
  { value: "denied", label: "拒绝" },
  { value: "failed", label: "失败" },
];

export const EXECUTION_STATE_OPTIONS: ReadonlyArray<{
  value: ExecutionState;
  label: string;
}> = [
  { value: "sent", label: "已发送" },
  { value: "running", label: "执行中" },
  { value: "succeeded", label: "执行成功" },
  { value: "failed", label: "执行失败" },
  { value: "timed_out", label: "执行超时" },
  { value: "unknown", label: "结果未知" },
];

const AUDIT_KIND_LABELS: Record<AuditKind, string> = {
  session_opened: "会话开启",
  session_closed: "会话关闭",
  command_executed: "命令执行",
  file_read: "文件读取",
  input_sent: "输入发送",
  output_read: "输出读取",
  permission_denied: "权限拒绝",
  connection_created: "连接创建",
  connection_updated: "连接更新",
  connection_deleted: "连接删除",
  connection_tested: "连接测试",
  token_created: "Token 创建",
  token_disabled: "Token 禁用",
  token_revoked: "Token 撤销",
  grant_changed: "授权变更",
  mcp_server_started: "MCP 服务启动",
  mcp_server_stopped: "MCP 服务停止",
};

const AUDIT_OUTCOME_LABELS: Record<AuditOutcome, string> = {
  allowed: "允许",
  denied: "拒绝",
  failed: "失败",
};

export function auditKindLabel(kind: AuditKind): string {
  return AUDIT_KIND_LABELS[kind];
}

export function auditOutcomeLabel(outcome: AuditOutcome): string {
  return AUDIT_OUTCOME_LABELS[outcome];
}

function emptyToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function parseDatetimeLocal(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed.length === 0) {
    return null;
  }
  const parsed = new Date(trimmed).getTime();
  return Number.isFinite(parsed) ? parsed : null;
}

export function buildAuditQuery(
  draft: AuditFilterDraft,
  page: number,
  limit: number = AUDIT_PAGE_SIZE,
): AuditQueryDto {
  return {
    offset: Math.max(0, page) * limit,
    limit,
    tokenId: emptyToNull(draft.tokenId),
    connectionId: emptyToNull(draft.connectionId),
    sessionId: emptyToNull(draft.sessionId),
    kind: draft.kind === "" ? null : draft.kind,
    outcome: draft.outcome === "" ? null : draft.outcome,
    from: parseDatetimeLocal(draft.from),
    to: parseDatetimeLocal(draft.to),
  };
}

export function buildExportQuery(draft: AuditFilterDraft): AuditQueryDto {
  return buildAuditQuery(draft, 0, EXPORT_LIMIT);
}

export function describeAuditError(error: unknown, action: string): string {
  if (isApiError(error) && error.code === "not_implemented") {
    return `后端命令尚未接通：${action}接口尚未实现。`;
  }
  return toUserMessage(error);
}

export function formatDuration(ms?: number | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) {
    return "—";
  }
  if (ms < 1000) {
    return `${Math.round(ms)} ms`;
  }
  return `${(ms / 1000).toFixed(2)} s`;
}

export function summarizeCommand(command?: string | null, maxLength = 64): string {
  if (!command) {
    return "—";
  }
  const singleLine = command.replace(/\s+/g, " ").trim();
  if (singleLine.length === 0) {
    return "—";
  }
  return singleLine.length > maxLength
    ? `${singleLine.slice(0, maxLength)}…`
    : singleLine;
}

export function abbreviateId(id?: string | null, maxLength = 12): string {
  if (!id) {
    return "—";
  }
  return id.length > maxLength ? `${id.slice(0, maxLength - 1)}…` : id;
}

export function isSameQuery(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
