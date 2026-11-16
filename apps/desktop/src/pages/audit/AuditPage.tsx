import { Fragment, useMemo, useState } from "react";
import type { AuditDto } from "../../api/types";
import { exportAudit, listActiveSessions, queryAudit } from "../../api/client";
import EmptyState from "../../components/EmptyState";
import SectionCard from "../../components/SectionCard";
import { formatUnixMillis } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import AuditDetail from "./AuditDetail";
import AuditFilters from "./AuditFilters";
import type { AuditFilterDraft } from "./audit";
import {
  AUDIT_PAGE_SIZE,
  EMPTY_AUDIT_FILTER,
  abbreviateId,
  auditKindLabel,
  auditOutcomeLabel,
  buildAuditQuery,
  buildExportQuery,
  describeAuditError,
  isSameQuery,
  summarizeCommand,
} from "./audit";
import Pager from "./Pager";
import SessionHistory from "./SessionHistory";
import "./audit.css";

type TabKey = "audit" | "sessions";

type ExportFeedback = {
  tone: "success" | "error";
  message: string;
};

export default function AuditPage() {
  const [tab, setTab] = useState<TabKey>("audit");
  const [draft, setDraft] = useState<AuditFilterDraft>(EMPTY_AUDIT_FILTER);
  const [applied, setApplied] = useState<AuditFilterDraft>(EMPTY_AUDIT_FILTER);
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<AuditDto | null>(null);
  const [exporting, setExporting] = useState(false);
  const [feedback, setFeedback] = useState<ExportFeedback | null>(null);

  const query = useMemo(() => buildAuditQuery(applied, page), [applied, page]);
  const { data, loading, error, reload } = useAsync(
    () => queryAudit(query),
    [query],
  );
  const activeSessions = useAsync(() => listActiveSessions(), []);

  const activeSessionCount = activeSessions.data?.length ?? 0;
  const total = data?.total ?? 0;
  const records = useMemo(() => {
    const items = data?.items ?? [];
    return applied.state === ""
      ? items
      : items.filter((item) => item.state === applied.state);
  }, [data, applied.state]);

  const handleApply = () => {
    const nextQuery = buildAuditQuery(draft, 0);
    setApplied(draft);
    setPage(0);
    setSelected(null);
    setFeedback(null);
    if (isSameQuery(nextQuery, query)) {
      reload();
    }
  };

  const handleReset = () => {
    const empty = { ...EMPTY_AUDIT_FILTER };
    const nextQuery = buildAuditQuery(empty, 0);
    setDraft(empty);
    setApplied(empty);
    setPage(0);
    setSelected(null);
    setFeedback(null);
    if (isSameQuery(nextQuery, query)) {
      reload();
    }
  };

  const handleExport = () => {
    setExporting(true);
    setFeedback(null);
    void exportAudit(buildExportQuery(applied)).then(
      (result) => {
        setExporting(false);
        setFeedback({
          tone: "success",
          message: `已导出 ${result.rows} 行，文件：${result.path}`,
        });
      },
      (exportError: unknown) => {
        setExporting(false);
        setFeedback({
          tone: "error",
          message: describeAuditError(exportError, "审计导出"),
        });
      },
    );
  };

  const handlePageChange = (nextPage: number) => {
    setPage(nextPage);
    setSelected(null);
  };

  let content;
  if (loading) {
    content = <p className="audit-hint">正在加载审计记录…</p>;
  } else if (error) {
    content = (
      <p className="audit-error" role="alert">
        {describeAuditError(error, "审计查询")}
      </p>
    );
  } else if (records.length === 0) {
    content = (
      <EmptyState
        title="没有匹配的审计记录"
        description="调整筛选条件后重新查询。"
      />
    );
  } else {
    content = (
      <>
        <div className="audit-table-wrap">
          <table className="audit-table">
            <thead>
              <tr>
                <th>时间</th>
                <th>操作</th>
                <th>结果</th>
                <th>Token</th>
                <th>连接</th>
                <th>Session</th>
                <th>命令摘要</th>
              </tr>
            </thead>
            <tbody>
              {records.map((audit) => {
                const isSelected = selected?.id === audit.id;
                const toggle = () => setSelected(isSelected ? null : audit);
                return (
                  <Fragment key={audit.id}>
                    <tr
                      className={`audit-row${isSelected ? " audit-row--selected" : ""}`}
                      tabIndex={0}
                      onClick={toggle}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          toggle();
                        }
                      }}
                    >
                      <td className="audit-cell--nowrap">
                        {formatUnixMillis(audit.startedAt)}
                      </td>
                      <td>{auditKindLabel(audit.kind)}</td>
                      <td>
                        <span className={`audit-outcome audit-outcome--${audit.outcome}`}>
                          {auditOutcomeLabel(audit.outcome)}
                        </span>
                      </td>
                      <td title={audit.tokenId ?? undefined}>
                        {abbreviateId(audit.tokenId, 10)}
                      </td>
                      <td title={audit.connectionId ?? undefined}>
                        {abbreviateId(audit.connectionId, 10)}
                      </td>
                      <td title={audit.sessionId ?? undefined}>
                        {abbreviateId(audit.sessionId, 10)}
                      </td>
                      <td>
                        <span
                          className="audit-cell__command"
                          title={audit.command ?? undefined}
                        >
                          {summarizeCommand(audit.command)}
                        </span>
                      </td>
                    </tr>
                    {isSelected ? (
                      <tr className="audit-detail-row">
                        <td colSpan={7}>
                          <AuditDetail
                            audit={audit}
                            onClose={() => setSelected(null)}
                          />
                        </td>
                      </tr>
                    ) : null}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
        <Pager
          page={page}
          total={total}
          pageSize={AUDIT_PAGE_SIZE}
          onPageChange={handlePageChange}
        />
      </>
    );
  }

  const description =
    applied.state === ""
      ? `共 ${total} 条记录，每页 ${AUDIT_PAGE_SIZE} 条。`
      : `共 ${total} 条记录 · 当前页符合执行状态筛选的有 ${records.length} 条。`;

  return (
    <section className="page audit-page">
      <header className="audit-header">
        <h1>审计日志</h1>
        <p>查看 MCP 会话与命令执行记录，支持筛选、详情与导出。</p>
      </header>

      <div className="audit-tabs" role="tablist" aria-label="审计视图">
        <button
          type="button"
          role="tab"
          id="audit-tab-records"
          aria-selected={tab === "audit"}
          aria-controls="audit-panel-records"
          className={`audit-tab${tab === "audit" ? " audit-tab--active" : ""}`}
          onClick={() => setTab("audit")}
        >
          命令审计
        </button>
        <button
          type="button"
          role="tab"
          id="audit-tab-sessions"
          aria-selected={tab === "sessions"}
          aria-controls="audit-panel-sessions"
          className={`audit-tab${tab === "sessions" ? " audit-tab--active" : ""}`}
          onClick={() => setTab("sessions")}
        >
          会话历史
          {activeSessionCount > 0 ? (
            <span className="audit-tab__badge" aria-label="活跃会话数">
              {activeSessionCount}
            </span>
          ) : null}
        </button>
      </div>

      {tab === "audit" ? (
        <div
          role="tabpanel"
          id="audit-panel-records"
          aria-labelledby="audit-tab-records"
          className="audit-panel"
        >
          <SectionCard
            title="筛选条件"
            description="按时间范围、Token、连接、会话与执行结果筛选审计记录。"
            actions={
              <>
                <button
                  type="button"
                  className="btn btn--primary"
                  onClick={handleApply}
                >
                  查询
                </button>
                <button type="button" className="btn btn--ghost" onClick={handleReset}>
                  重置
                </button>
                <button
                  type="button"
                  className="btn btn--ghost"
                  onClick={handleExport}
                  disabled={exporting}
                >
                  {exporting ? "导出中…" : "导出 CSV"}
                </button>
              </>
            }
          >
            <AuditFilters value={draft} onChange={setDraft} />
            {feedback ? (
              <p
                className={`audit-feedback audit-feedback--${feedback.tone}`}
                role={feedback.tone === "error" ? "alert" : "status"}
              >
                {feedback.message}
              </p>
            ) : null}
          </SectionCard>

          <SectionCard
            title="命令审计"
            description={description}
            actions={
              <button
                type="button"
                className="btn btn--ghost btn--small"
                onClick={reload}
                disabled={loading}
              >
                刷新
              </button>
            }
          >
            {content}
          </SectionCard>
        </div>
      ) : (
        <div
          role="tabpanel"
          id="audit-panel-sessions"
          aria-labelledby="audit-tab-sessions"
          className="audit-panel"
        >
          <SessionHistory />
        </div>
      )}
    </section>
  );
}
