import { useState } from "react";
import { listTokenGrants, setTokenGrant } from "../../api/client";
import type { GrantDto, TokenDto } from "../../api/types";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";

export type TokenDetailPanelProps = {
  token: TokenDto;
  onClose: () => void;
};

function maskToken(prefix: string): string {
  return `umt_${prefix}_••••••`;
}

function formatExpiry(expiresAt?: number | null): string {
  return expiresAt === null || expiresAt === undefined
    ? "永久有效"
    : formatUnixMillis(expiresAt);
}

function formatLastUsed(lastUsedAt?: number | null): string {
  return lastUsedAt === null || lastUsedAt === undefined
    ? "从未使用"
    : formatUnixMillis(lastUsedAt);
}

export default function TokenDetailPanel({ token, onClose }: TokenDetailPanelProps) {
  const grantsQuery = useAsync(() => listTokenGrants(token.id), [token.id]);
  const [overrides, setOverrides] = useState<Record<string, GrantDto>>({});
  const [savingConnectionId, setSavingConnectionId] = useState<string | null>(null);
  const [grantError, setGrantError] = useState<string | null>(null);

  const baseGrants = grantsQuery.data ?? [];
  const grants = baseGrants.map((grant) => overrides[grant.connection.id] ?? grant);

  const handleToggle = async (grant: GrantDto) => {
    const connectionId = grant.connection.id;
    const nextAllowed = !grant.allowed;
    setGrantError(null);
    setOverrides((previous) => ({
      ...previous,
      [connectionId]: {
        ...grant,
        allowed: nextAllowed,
        grantedAt: nextAllowed ? Date.now() : null,
      },
    }));
    setSavingConnectionId(connectionId);

    try {
      const updated = await setTokenGrant({
        tokenId: token.id,
        connectionId,
        allowed: nextAllowed,
      });
      setOverrides((previous) => ({ ...previous, [connectionId]: updated }));
    } catch (error) {
      setOverrides((previous) => {
        if (!Object.hasOwn(previous, connectionId)) {
          return previous;
        }
        const next = { ...previous };
        delete next[connectionId];
        return next;
      });
      setGrantError(toUserMessage(error));
    } finally {
      setSavingConnectionId(null);
    }
  };

  return (
    <aside className="access-control__detail" aria-label={`${token.name} 详情`}>
      <header className="detail-header">
        <div className="detail-header__heading">
          <h2 className="detail-header__title">{token.name}</h2>
          <code className="detail-header__token">{maskToken(token.prefix)}</code>
        </div>
        <button
          type="button"
          className="btn btn--ghost btn--small"
          onClick={onClose}
          aria-label="关闭详情"
        >
          关闭
        </button>
      </header>

      <dl className="detail-meta">
        <div className="detail-meta__item">
          <dt>状态</dt>
          <dd>
            <StatusBadge status={token.status} />
          </dd>
        </div>
        <div className="detail-meta__item">
          <dt>有效期</dt>
          <dd>{formatExpiry(token.expiresAt)}</dd>
        </div>
        <div className="detail-meta__item">
          <dt>创建时间</dt>
          <dd>{formatUnixMillis(token.createdAt)}</dd>
        </div>
        <div className="detail-meta__item">
          <dt>最后使用</dt>
          <dd>{formatLastUsed(token.lastUsedAt)}</dd>
        </div>
      </dl>

      {token.description ? (
        <p className="detail-description">{token.description}</p>
      ) : null}

      <section className="grants">
        <div className="grants__heading">
          <h3 className="grants__title">连接授权</h3>
          <span className="grants__count">共 {grants.length} 个连接</span>
        </div>
        <p className="grants__hint">关闭授权会立即阻止该 Token 的新操作。</p>

        {grantError ? (
          <div className="access-control__alert access-control__alert--danger" role="alert">
            <span>{grantError}</span>
          </div>
        ) : null}

        {grantsQuery.loading && baseGrants.length === 0 ? (
          <p className="grants__state">正在加载授权矩阵…</p>
        ) : null}

        {grantsQuery.error ? (
          <div className="access-control__alert access-control__alert--danger" role="alert">
            <span>{toUserMessage(grantsQuery.error)}</span>
            <button
              type="button"
              className="btn btn--ghost btn--small"
              onClick={grantsQuery.reload}
            >
              重试
            </button>
          </div>
        ) : null}

        {!grantsQuery.loading && !grantsQuery.error && grants.length === 0 ? (
          <p className="grants__state">暂无已配置的连接，请先在连接管理中创建连接。</p>
        ) : null}

        {grants.length > 0 ? (
          <ul className="grant-list">
            {grants.map((grant) => {
              const saving = savingConnectionId === grant.connection.id;
              return (
                <li className="grant-item" key={grant.connection.id}>
                  <span className="grant-item__name" title={grant.connection.name}>
                    {grant.connection.name}
                  </span>
                  <span className="grant-item__protocol">
                    {grant.connection.protocolDisplayName}
                  </span>
                  <label className="grant-toggle">
                    <input
                      type="checkbox"
                      role="switch"
                      aria-label={`授权 ${grant.connection.name}`}
                      checked={grant.allowed}
                      disabled={savingConnectionId !== null}
                      onChange={() => void handleToggle(grant)}
                    />
                    <span>{grant.allowed ? "已授权" : "未授权"}</span>
                  </label>
                  <span className="grant-item__time">
                    {saving ? "保存中…" : formatUnixMillis(grant.grantedAt)}
                  </span>
                </li>
              );
            })}
          </ul>
        ) : null}
      </section>
    </aside>
  );
}
