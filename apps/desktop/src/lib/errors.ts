import type { ApiError } from "../api/types";

const GENERIC_MESSAGE = "操作失败，请稍后重试。";

const CODE_MESSAGES: Record<string, string> = {
  access_denied: "没有权限执行此操作。",
  validation: "提交的内容不合法，请检查后重试。",
  not_found: "目标资源不存在，可能已被删除。",
  conflict: "数据已被其他操作修改，请刷新后重试。",
  not_implemented: "该功能尚未实现。",
};

export function isApiError(value: unknown): value is ApiError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as { code?: unknown; message?: unknown };
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

function cleanDetail(raw: string): string | null {
  const line = raw
    .split(/\r?\n/)
    .map((part) => part.trim())
    .find((part) => part.length > 0);
  if (!line) {
    return null;
  }
  if (/^at\s+\S+/.test(line) || line.startsWith("Error:")) {
    return null;
  }
  return line.length > 120 ? `${line.slice(0, 120)}…` : line;
}

function extractDetail(error: unknown): string | null {
  if (typeof error === "string") {
    return cleanDetail(error);
  }
  if (error instanceof Error) {
    return cleanDetail(error.message);
  }
  if (typeof error === "object" && error !== null) {
    const candidate = error as { code?: unknown; message?: unknown };
    if (isApiError(candidate)) {
      return cleanDetail(candidate.message);
    }
    if (typeof candidate.message === "string") {
      return cleanDetail(candidate.message);
    }
  }
  return null;
}

export function toUserMessage(error: unknown): string {
  const code = isApiError(error) ? error.code : null;
  const base = (code && CODE_MESSAGES[code]) ?? GENERIC_MESSAGE;
  const detail = extractDetail(error);
  return detail ? `${base}（${detail}）` : base;
}

function pad(value: number, length = 2): string {
  return String(value).padStart(length, "0");
}

export function formatUnixMillis(ms?: number | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) {
    return "—";
  }
  const date = new Date(ms);
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
  ].join(" ");
}
