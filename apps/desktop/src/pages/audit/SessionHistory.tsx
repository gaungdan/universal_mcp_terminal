import { useMemo, useState } from "react";
import type { SessionDto, SessionFilterDto } from "../../api/types";
import { listSessionHistory } from "../../api/client";
import EmptyState from "../../components/EmptyState";
import SectionCard from "../../components/SectionCard";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import { abbreviateId, describeAuditError, isSameQuery } from "./audit";
import Pager from "./Pager";

const SESSION_PAGE_SIZE = 50;

type SessionDraft = {
  tokenId: string;
  connectionId: string;
};

const EMPTY_SESSION_FILTER: SessionDraft = {
  tokenId: "",
  connectionId: "",
};

function buildSessionQuery(draft: SessionDraft, page: number): SessionFilterDto {
  return {
    offset: Math.max(0, page) * SESSION_PAGE_SIZE,
    limit: SESSION_PAGE_SIZE,
    tokenId: draft.tokenId.trim() || null,
    connectionId: draft.connectionId.trim() || null,
  };
}

export default function SessionHistory() {
  const [draft, setDraft] = useState<SessionDraft>(EMPTY_SESSION_FILTER);
  const [applied, setApplied] = useState<SessionDraft>(EMPTY_SESSION_FILTER);
  const [page, setPage] = useState(0);

  const query = useMemo(() => buildSessionQuery(applied, page), [applied, page]);
  const { data, loading, error, reload } = useAsync(
    () => listSessionHistory(query),
    [query],
  );

  const total = data?.total ?? 0;
  const sessions: SessionDto[] = data?.items ?? [];

  const handleApply = () => {
    const nextQuery = buildSessionQuery(draft, 0);
    setApplied(draft);
    setPage(0);
    if (isSameQuery(nextQuery, query)) {
      reload();
    }
  };

  const handleReset = () => {
    const empty = { ...EMPTY_SESSION_FILTER };
    const nextQuery = buildSessionQuery(empty, 0);
    setDraft(empty);
    setApplied(empty);
    setPage(0);
    if (isSameQuery(nextQuery, query)) {
      reload();
    }
  };

  let content;
  if (loading) {
    content = <p className="audit-hint">正在加载会话历史…</p>;
  } else if (error) {
    content = (
      <p className="audit-error" role="alert">
        {describeAuditError(error, "会话历史查询")}
      </p>
    );
  } else if (sessions.length === 0) {
    content = (
      <EmptyState
        title="没有匹配的会话记录"
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
                <th>Session</th>
                <th>状态</th>
                <th>Token</th>
                <th>连接</th>
                <th>开始时间</th>
                <th>结束时间</th>
                <th>关闭原因</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session) => (
                <tr key={session.id} className="audit-row audit-row--static">
                  <td title={session.id}>{abbreviateId(session.id, 16)}</td>
                  <td>
                    <StatusBadge status={session.state} />
                  </td>
                  <td title={session.tokenId}>{abbreviateId(session.tokenId)}</td>
                  <td title={session.connectionId}>
                    {abbreviateId(session.connectionId)}
                  </td>
                  <td className="audit-cell--nowrap">
                    {formatUnixMillis(session.startedAt)}
                  </td>
                  <td className="audit-cell--nowrap">
                    {formatUnixMillis(session.endedAt)}
                  </td>
                  <td className="audit-cell--reason">{session.closeReason ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <Pager
          page={page}
          total={total}
          pageSize={SESSION_PAGE_SIZE}
          onPageChange={setPage}
        />
      </>
    );
  }

  return (
    <SectionCard
      title="会话历史"
      description={`共 ${total} 条会话记录，每页 ${SESSION_PAGE_SIZE} 条。`}
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
      <div className="audit-session-filters">
        <div className="form-field">
          <label className="form-field__label" htmlFor="audit-session-token">
            Token ID
          </label>
          <input
            id="audit-session-token"
            className="form-field__control"
            type="text"
            placeholder="按 Token 筛选"
            value={draft.tokenId}
            onChange={(event) =>
              setDraft((previous) => ({ ...previous, tokenId: event.target.value }))
            }
          />
        </div>
        <div className="form-field">
          <label className="form-field__label" htmlFor="audit-session-connection">
            Connection ID
          </label>
          <input
            id="audit-session-connection"
            className="form-field__control"
            type="text"
            placeholder="按连接筛选"
            value={draft.connectionId}
            onChange={(event) =>
              setDraft((previous) => ({
                ...previous,
                connectionId: event.target.value,
              }))
            }
          />
        </div>
        <div className="audit-session-filters__actions">
          <button type="button" className="btn btn--primary" onClick={handleApply}>
            查询
          </button>
          <button type="button" className="btn btn--ghost" onClick={handleReset}>
            重置
          </button>
        </div>
      </div>
      {content}
    </SectionCard>
  );
}
