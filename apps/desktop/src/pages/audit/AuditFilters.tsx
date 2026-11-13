import { useId } from "react";
import type { AuditKind, AuditOutcome, ExecutionState } from "../../api/types";
import type { AuditFilterDraft } from "./audit";
import {
  AUDIT_KIND_OPTIONS,
  AUDIT_OUTCOME_OPTIONS,
  EXECUTION_STATE_OPTIONS,
} from "./audit";

export type AuditFiltersProps = {
  value: AuditFilterDraft;
  onChange: (next: AuditFilterDraft) => void;
};

export default function AuditFilters({ value, onChange }: AuditFiltersProps) {
  const rawId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const fieldId = (name: string) => `audit-filter-${rawId}-${name}`;

  function update<K extends keyof AuditFilterDraft>(
    key: K,
    next: AuditFilterDraft[K],
  ) {
    onChange({ ...value, [key]: next });
  }

  return (
    <div className="audit-filters">
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("from")}>
          开始时间
        </label>
        <input
          id={fieldId("from")}
          className="form-field__control"
          type="datetime-local"
          value={value.from}
          onChange={(event) => update("from", event.target.value)}
        />
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("to")}>
          结束时间
        </label>
        <input
          id={fieldId("to")}
          className="form-field__control"
          type="datetime-local"
          value={value.to}
          onChange={(event) => update("to", event.target.value)}
        />
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("token")}>
          Token ID
        </label>
        <input
          id={fieldId("token")}
          className="form-field__control"
          type="text"
          placeholder="按 Token 筛选"
          value={value.tokenId}
          onChange={(event) => update("tokenId", event.target.value)}
        />
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("connection")}>
          Connection ID
        </label>
        <input
          id={fieldId("connection")}
          className="form-field__control"
          type="text"
          placeholder="按连接筛选"
          value={value.connectionId}
          onChange={(event) => update("connectionId", event.target.value)}
        />
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("session")}>
          Session ID
        </label>
        <input
          id={fieldId("session")}
          className="form-field__control"
          type="text"
          placeholder="按会话筛选"
          value={value.sessionId}
          onChange={(event) => update("sessionId", event.target.value)}
        />
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("kind")}>
          操作类型
        </label>
        <select
          id={fieldId("kind")}
          className="form-field__control"
          value={value.kind}
          onChange={(event) => update("kind", event.target.value as "" | AuditKind)}
        >
          <option value="">全部操作</option>
          {AUDIT_KIND_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("outcome")}>
          执行结果
        </label>
        <select
          id={fieldId("outcome")}
          className="form-field__control"
          value={value.outcome}
          onChange={(event) =>
            update("outcome", event.target.value as "" | AuditOutcome)
          }
        >
          <option value="">全部结果</option>
          {AUDIT_OUTCOME_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
      <div className="form-field">
        <label className="form-field__label" htmlFor={fieldId("state")}>
          执行状态
        </label>
        <select
          id={fieldId("state")}
          className="form-field__control"
          value={value.state}
          onChange={(event) =>
            update("state", event.target.value as "" | ExecutionState)
          }
        >
          <option value="">全部状态</option>
          {EXECUTION_STATE_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <p className="form-field__help">执行状态仅在当前页记录内过滤。</p>
      </div>
    </div>
  );
}
