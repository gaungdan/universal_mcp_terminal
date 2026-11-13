import type { SecretInput } from "../../api/types";
import { isApiError } from "../../lib/errors";

export const CONFLICT_MESSAGE = "连接已被其他操作修改，请刷新后重试。";

export function parseTags(raw: string): string[] {
  const tags = new Set<string>();
  for (const part of raw.split(/[,，]/)) {
    const tag = part.trim();
    if (tag.length > 0) {
      tags.add(tag);
    }
  }
  return [...tags];
}

export function toSecretInputs(secrets: Record<string, string>): SecretInput[] {
  return Object.entries(secrets)
    .filter((entry) => entry[1].length > 0)
    .map(([key, value]) => ({ key, value }));
}

export function isConflictError(error: unknown): boolean {
  return isApiError(error) && error.code === "conflict";
}

export function formatConfigValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "—";
  }
  if (typeof value === "boolean") {
    return value ? "是" : "否";
  }
  if (typeof value === "string") {
    return value.trim() === "" ? "—" : value;
  }
  if (typeof value === "number") {
    return String(value);
  }
  if (Array.isArray(value)) {
    return value.map((item) => formatConfigValue(item)).join("、");
  }
  return JSON.stringify(value) ?? "—";
}
