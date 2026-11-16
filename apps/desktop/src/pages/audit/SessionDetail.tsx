import type { SessionDto } from "../../api/types";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis } from "../../lib/errors";
import { abbreviateId } from "./audit";

export type SessionDetailProps = {
  session: SessionDto;
  onClose: () => void;
};

function formatDuration(session: SessionDto): string {
  if (session.endedAt === null || session.endedAt === undefined) {
    return "进行中";
  }
  const ms = Math.max(0, session.endedAt - session.startedAt);
  if (ms < 1000) {
    return `${ms} ms`;
  }
  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(2)} s`;
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes} 分 ${Math.round(seconds % 60)} 秒`;
}

export default function SessionDetail({ session, onClose }: SessionDetailProps) {
  return (
    <section className="audit-detail session-detail" aria-label="会话详情">
      <header className="audit-detail__header">
        <div>
          <h3 className="audit-detail__title">
            会话详情 {abbreviateId(session.id, 16)}
          </h3>
          <p className="audit-detail__subtitle">
            {formatUnixMillis(session.startedAt)} · {formatDuration(session)}
          </p>
        </div>
        <div className="audit-detail__actions">
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
        <dl className="audit-meta">
          <div>
            <dt>状态</dt>
            <dd>
              <StatusBadge status={session.state} />
            </dd>
          </div>
          <div>
            <dt>协议</dt>
            <dd>{session.protocolId || "—"}</dd>
          </div>
          <div>
            <dt>Token</dt>
            <dd className="session-detail__mono">{session.tokenId}</dd>
          </div>
          <div>
            <dt>连接</dt>
            <dd className="session-detail__mono">{session.connectionId}</dd>
          </div>
          <div>
            <dt>开始时间</dt>
            <dd>{formatUnixMillis(session.startedAt)}</dd>
          </div>
          <div>
            <dt>结束时间</dt>
            <dd>{formatUnixMillis(session.endedAt)}</dd>
          </div>
          <div>
            <dt>关闭原因</dt>
            <dd>{session.closeReason ?? "—"}</dd>
          </div>
          <div>
            <dt>终端尺寸</dt>
            <dd>
              {session.rows} × {session.cols}
            </dd>
          </div>
          <div>
            <dt>客户端</dt>
            <dd>{session.clientInfo ?? "—"}</dd>
          </div>
          <div>
            <dt>Session ID</dt>
            <dd className="session-detail__mono">{session.id}</dd>
          </div>
        </dl>
      </div>
    </section>
  );
}
