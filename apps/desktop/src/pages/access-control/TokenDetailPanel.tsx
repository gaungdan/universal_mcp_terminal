import { useState } from "react";
import {
  listTokenGrants,
  setTokenGrant,
  updateToken,
} from "../../api/client";
import type {
  GrantDto,
  TokenDto,
  TokenPermission,
} from "../../api/types";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";

export type TokenDetailPanelProps = {
  token: TokenDto;
  onClose: () => void;
  onUpdated?: (token: TokenDto) => void;
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

export default function TokenDetailPanel({
  token,
  onClose,
  onUpdated,
}: TokenDetailPanelProps) {
  const grantsQuery = useAsync(() => listTokenGrants(token.id), [token.id]);
  const [overrides, setOverrides] = useState<Record<string, GrantDto>>({});
  const [savingConnectionId, setSavingConnectionId] = useState<string | null>(null);
  const [grantError, setGrantError] = useState<string | null>(null);
  const [permission, setPermission] = useState<TokenPermission>(token.permission);
  const [allowInteractive, setAllowInteractive] = useState(token.allowInteractive);
  const [savingPermission, setSavingPermission] = useState(false);
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const [permissionSaved, setPermissionSaved] = useState(false);

  const changePermission = (next: TokenPermission) => {
    setPermission(next);
    setPermissionSaved(false);
    if (next === "read_only") {
      setAllowInteractive(false);
    }
  };

  const handleSavePermission = async () => {
    setSavingPermission(true);
    setPermissionError(null);
    setPermissionSaved(false);
    try {
      const updated = await updateToken({
        id: token.id,
        name: token.name,
        description: token.description ?? null,
        permission,
        allowInteractive,
        expiresAt: token.expiresAt ?? null,
      });
      onUpdated?.(updated);
      setPermissionSaved(true);
    } catch (error) {
      setPermissionError(toUserMessage(error));
    } finally {
      setSavingPermission(false);
    }
  };

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
    <section className="access-control__detail" aria-label={`${token.name} 详情`}>
      <header className="detail-header">
        <div className="detail-header__heading">
          <div className="detail-header__title-row">
            <h3 className="detail-header__title">{token.name}</h3>
            <code className="detail-header__token">{maskToken(token.prefix)}</code>
          </div>
          {token.description ? (
            <p className="detail-header__description">{token.description}</p>
          ) : null}
        </div>
        <button
          type="button"
          className="btn btn--ghost btn--small"
          onClick={onClose}
          aria-label="关闭详情"
        >
          收起
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
          <dt>权限</dt>
          <dd>
            {token.permission === "write_access" ? "WriteAccess" : "ReadOnly"}
            {token.allowInteractive ? " · 交互式终端" : ""}
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

      <section className="permission-editor" aria-label="权限设置">
        <div className="grants__heading">
          <h4 className="grants__title">权限设置</h4>
          {permissionSaved ? (
            <span className="permission-editor__saved">已保存</span>
          ) : null}
        </div>
        <div className="permission-options" role="radiogroup" aria-label="Token 权限">
          <label className="permission-option">
            <input
              type="radio"
              name={`permission-${token.id}`}
              value="read_only"
              checked={permission === "read_only"}
              disabled={savingPermission || token.status === "revoked"}
              onChange={() => changePermission("read_only")}
            />
            <span>
              <strong>ReadOnly</strong>
              <small>仅允许查询类命令。</small>
            </span>
          </label>
          <label className="permission-option">
            <input
              type="radio"
              name={`permission-${token.id}`}
              value="write_access"
              checked={permission === "write_access"}
              disabled={savingPermission || token.status === "revoked"}
              onChange={() => changePermission("write_access")}
            />
            <span>
              <strong>WriteAccess</strong>
              <small>允许普通写操作，禁止提权与灾难命令。</small>
            </span>
          </label>
        </div>
        <label className="form-field__switch">
          <input
            type="checkbox"
            checked={allowInteractive}
            disabled={
              savingPermission ||
              token.status === "revoked" ||
              permission === "read_only"
            }
            onChange={(event) => {
              setAllowInteractive(event.target.checked);
              setPermissionSaved(false);
            }}
          />
          <span>
            允许交互式终端
            <small>
              {permission === "read_only"
                ? "ReadOnly 不支持交互式终端。"
                : "交互式终端等同于完整 Shell，无法逐条过滤命令。"}
            </small>
          </span>
        </label>
        {permission === "write_access" ? (
          <p className="form-field__warning" role="note">
            WriteAccess 可修改文件和系统配置，请只授权给可信的 AI 客户端。
          </p>
        ) : null}
        {permissionError ? (
          <p className="form-field__error" role="alert">
            {permissionError}
          </p>
        ) : null}
        <div className="permission-editor__actions">
          <button
            type="button"
            className="btn btn--primary btn--small"
            onClick={() => void handleSavePermission()}
            disabled={
              savingPermission ||
              token.status === "revoked" ||
              (permission === token.permission &&
                allowInteractive === token.allowInteractive)
            }
          >
            {savingPermission ? "保存中…" : "保存权限"}
          </button>
        </div>
      </section>

      <section className="grants">
        <div className="grants__heading">
          <h4 className="grants__title">连接授权</h4>
          <span className="grants__count">{grants.length} 个连接</span>
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
          <p className="grants__state">
            暂无已配置的连接，请先在连接管理中创建连接。
          </p>
        ) : null}

        {grants.length > 0 ? (
          <ul className="grant-list">
            {grants.map((grant) => {
              const saving = savingConnectionId === grant.connection.id;
              return (
                <li className="grant-item" key={grant.connection.id}>
                  <div className="grant-item__identity">
                    <span className="grant-item__name" title={grant.connection.name}>
                      {grant.connection.name}
                    </span>
                    <span className="grant-item__protocol">
                      {saving
                        ? "保存中…"
                        : grant.allowed && grant.grantedAt
                          ? `${grant.connection.protocolDisplayName} · ${formatUnixMillis(grant.grantedAt)}`
                          : grant.connection.protocolDisplayName}
                    </span>
                  </div>
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
                </li>
              );
            })}
          </ul>
        ) : null}
      </section>
    </section>
  );
}
