import { useState } from "react";
import type { AuditDto } from "../../api/types";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis } from "../../lib/errors";
import {
  auditKindLabel,
  auditOutcomeLabel,
  formatDuration,
} from "./audit";

export type AuditDetailProps = {
  audit: AuditDto;
  onClose: () => void;
};

export default function AuditDetail({ audit, onClose }: AuditDetailProps) {
  const [copyStatus, setCopyStatus] = useState<string | null>(null);

  const handleCopy = () => {
    const command = audit.command;
    if (!command) {
      setCopyStatus("该记录没有命令内容");
      return;
    }
    if (!navigator.clipboard?.writeText) {
      setCopyStatus("当前环境不支持剪贴板");
      return;
    }
    void navigator.clipboard.writeText(command).then(
      () => setCopyStatus("已复制命令"),
      () => setCopyStatus("复制失败，请手动选择命令"),
    );
  };

  return (
    <section className="audit-detail" aria-label="审计详情">
      <header className="audit-detail__header">
        <div>
          <h3 className="audit-detail__title">审计详情 #{audit.id}</h3>
          <p className="audit-detail__subtitle">
            {formatUnixMillis(audit.startedAt)}
          </p>
        </div>
        <div className="audit-detail__actions">
          <button
            type="button"
            className="btn btn--ghost btn--small"
            onClick={handleCopy}
            disabled={!audit.command}
          >
            复制命令
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            onClick={onClose}
            aria-label="关闭详情"
          >
            收起
          </button>
        </div>
      </header>
      <div className="audit-detail__body">
        {audit.redacted ? (
          <p className="audit-detail__redacted">已脱敏</p>
        ) : null}
        {copyStatus ? (
          <p className="audit-detail__copy-status" role="status">
            {copyStatus}
          </p>
        ) : null}
        <dl className="audit-meta">
          <div>
            <dt>操作类型</dt>
            <dd>{auditKindLabel(audit.kind)}</dd>
          </div>
          <div>
            <dt>MCP 工具</dt>
            <dd>{audit.toolName ?? "—"}</dd>
          </div>
          <div>
            <dt>权限</dt>
            <dd>
              {audit.permission === "write_access"
                ? "WriteAccess"
                : audit.permission === "read_only"
                  ? "ReadOnly"
                  : "—"}
            </dd>
          </div>
          <div>
            <dt>执行结果</dt>
            <dd>
              <span className={`audit-outcome audit-outcome--${audit.outcome}`}>
                {auditOutcomeLabel(audit.outcome)}
              </span>
            </dd>
          </div>
          <div>
            <dt>执行状态</dt>
            <dd>
              <StatusBadge status={audit.state} />
            </dd>
          </div>
          <div>
            <dt>耗时</dt>
            <dd>{formatDuration(audit.durationMs)}</dd>
          </div>
          <div>
            <dt>退出码</dt>
            <dd>{audit.exitCode === undefined || audit.exitCode === null ? "—" : audit.exitCode}</dd>
          </div>
          <div>
            <dt>工作目录</dt>
            <dd>{audit.workingDir ?? "—"}</dd>
          </div>
          <div>
            <dt>Token</dt>
            <dd>{audit.tokenId ?? "—"}</dd>
          </div>
          <div>
            <dt>连接</dt>
            <dd>{audit.connectionId ?? "—"}</dd>
          </div>
          <div>
            <dt>Session</dt>
            <dd>{audit.sessionId ?? "—"}</dd>
          </div>
        </dl>
        <section className="audit-detail__section">
          <h3>完整命令</h3>
          <pre className="audit-code">{audit.command ?? "—"}</pre>
        </section>
        <section className="audit-detail__section">
          <h3>错误原因</h3>
          <p className="audit-detail__error-text">{audit.error ?? "—"}</p>
        </section>
        <details className="audit-output" open>
          <summary>标准输出</summary>
          <pre className="audit-code audit-code--output">
            {audit.stdoutExcerpt ?? "（无输出）"}
          </pre>
        </details>
        <details className="audit-output" open>
          <summary>标准错误</summary>
          <pre className="audit-code audit-code--output">
            {audit.stderrExcerpt ?? "（无输出）"}
          </pre>
        </details>
      </div>
    </section>
  );
}
