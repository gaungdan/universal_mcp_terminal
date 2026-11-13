import { useState } from "react";
import {
  createToken,
  deleteToken,
  listTokens,
  setTokenStatus,
} from "../../api/client";
import type {
  CreatedTokenDto,
  CreateTokenInput,
  TokenDto,
  TokenStatus,
} from "../../api/types";
import ConfirmDialog from "../../components/ConfirmDialog";
import EmptyState from "../../components/EmptyState";
import SectionCard from "../../components/SectionCard";
import StatusBadge from "../../components/StatusBadge";
import Toolbar from "../../components/Toolbar";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import CreateTokenDialog from "./CreateTokenDialog";
import TokenDetailPanel from "./TokenDetailPanel";
import TokenRevealDialog from "./TokenRevealDialog";
import "./access-control.css";

const PAGE_SIZE = 10;

type PendingAction = {
  kind: "revoke" | "delete";
  token: TokenDto;
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

export default function AccessControlPage() {
  const [offset, setOffset] = useState(0);
  const [selectedToken, setSelectedToken] = useState<TokenDto | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [revealed, setRevealed] = useState<CreatedTokenDto | null>(null);
  const [pendingAction, setPendingAction] = useState<PendingAction | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const tokensQuery = useAsync(
    () => listTokens({ offset, limit: PAGE_SIZE }),
    [offset],
  );
  const tokens = tokensQuery.data?.items ?? [];
  const total = tokensQuery.data?.total ?? 0;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const currentPage = Math.floor(offset / PAGE_SIZE) + 1;

  const syncToken = (updated: TokenDto) => {
    setSelectedToken((previous) =>
      previous && previous.id === updated.id ? updated : previous,
    );
  };

  const handleChangeStatus = async (token: TokenDto, status: TokenStatus) => {
    setActionBusy(true);
    setActionError(null);
    try {
      const updated = await setTokenStatus({ id: token.id, status });
      syncToken(updated);
      tokensQuery.reload();
    } catch (error) {
      setActionError(toUserMessage(error));
    } finally {
      setActionBusy(false);
    }
  };

  const handleCreate = async (input: CreateTokenInput) => {
    setCreateBusy(true);
    setCreateError(null);
    try {
      const created = await createToken(input);
      setCreateOpen(false);
      setRevealed(created);
    } catch (error) {
      setCreateError(toUserMessage(error));
    } finally {
      setCreateBusy(false);
    }
  };

  const handleCloseReveal = () => {
    setRevealed(null);
    if (offset !== 0) {
      setOffset(0);
    } else {
      tokensQuery.reload();
    }
  };

  const handleConfirmAction = async (action: PendingAction) => {
    setActionBusy(true);
    setActionError(null);
    try {
      if (action.kind === "revoke") {
        const updated = await setTokenStatus({ id: action.token.id, status: "revoked" });
        syncToken(updated);
        tokensQuery.reload();
      } else {
        const deleted = await deleteToken(action.token.id);
        if (!deleted) {
          setActionError("目标资源不存在，可能已被删除。");
        } else {
          if (selectedToken?.id === action.token.id) {
            setSelectedToken(null);
          }
          if (tokens.length === 1 && offset > 0) {
            setOffset(Math.max(0, offset - PAGE_SIZE));
          } else {
            tokensQuery.reload();
          }
        }
      }
    } catch (error) {
      setActionError(toUserMessage(error));
    } finally {
      setActionBusy(false);
      setPendingAction(null);
    }
  };

  const openCreate = () => {
    setCreateError(null);
    setCreateOpen(true);
  };

  const showEmpty =
    !tokensQuery.loading && !tokensQuery.error && tokens.length === 0;

  return (
    <section className="page access-control">
      <header className="access-control__header">
        <div>
          <h1>访问控制</h1>
          <p>管理 MCP 访问 Token，并为每个 Token 配置可访问的连接。</p>
        </div>
        <button type="button" className="btn btn--primary" onClick={openCreate}>
          新建 Token
        </button>
      </header>

      <div
        className={`access-control__layout${
          selectedToken ? " access-control__layout--with-detail" : ""
        }`}
      >
        <div className="access-control__main">
          <SectionCard
            title="Token 列表"
            description="Token 明文仅在创建时展示一次，之后只显示前缀。"
          >
            {actionError ? (
              <div className="access-control__alert access-control__alert--danger" role="alert">
                <span>{actionError}</span>
              </div>
            ) : null}

            {tokensQuery.error ? (
              <div className="access-control__alert access-control__alert--danger" role="alert">
                <span>{toUserMessage(tokensQuery.error)}</span>
                <button
                  type="button"
                  className="btn btn--ghost btn--small"
                  onClick={tokensQuery.reload}
                  disabled={tokensQuery.loading}
                >
                  重试
                </button>
              </div>
            ) : null}

            <Toolbar
              actions={
                <div className="access-control__pager">
                  <button
                    type="button"
                    className="btn btn--ghost btn--small"
                    onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
                    disabled={offset === 0 || tokensQuery.loading}
                  >
                    上一页
                  </button>
                  <span className="access-control__page-label">
                    第 {currentPage} / {pageCount} 页
                  </span>
                  <button
                    type="button"
                    className="btn btn--ghost btn--small"
                    onClick={() => setOffset(offset + PAGE_SIZE)}
                    disabled={offset + PAGE_SIZE >= total || tokensQuery.loading}
                  >
                    下一页
                  </button>
                </div>
              }
            >
              <span>共 {total} 条</span>
              <span className="access-control__hint">
                选择 Token 可查看连接授权矩阵
              </span>
            </Toolbar>

            {tokensQuery.loading && tokens.length === 0 ? (
              <p className="access-control__loading">正在加载 Token 列表…</p>
            ) : null}

            {showEmpty ? (
              <EmptyState
                title="还没有访问 Token"
                description="创建 Token 后即可授权 MCP 客户端访问已配置的连接。"
                action={
                  <button type="button" className="btn btn--primary" onClick={openCreate}>
                    创建第一个 Token
                  </button>
                }
              />
            ) : null}

            {tokens.length > 0 ? (
              <div className="access-control__table-wrap">
                <table className="access-control__table">
                  <thead>
                    <tr>
                      <th scope="col">名称</th>
                      <th scope="col">Token</th>
                      <th scope="col">状态</th>
                      <th scope="col">有效期</th>
                      <th scope="col">创建时间</th>
                      <th scope="col">最后使用</th>
                      <th scope="col">操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {tokens.map((token) => {
                      const selected = selectedToken?.id === token.id;
                      return (
                        <tr
                          key={token.id}
                          className={`access-control__row${
                            selected ? " access-control__row--selected" : ""
                          }`}
                          onClick={() => setSelectedToken(token)}
                        >
                          <td>
                            <div className="access-control__name">
                              <button
                                type="button"
                                className="link-button"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  setSelectedToken(token);
                                }}
                                aria-expanded={selected}
                              >
                                {token.name}
                              </button>
                              {token.description ? (
                                <span className="access-control__subtext">
                                  {token.description}
                                </span>
                              ) : null}
                            </div>
                          </td>
                          <td>
                            <code className="access-control__masked">
                              {maskToken(token.prefix)}
                            </code>
                          </td>
                          <td>
                            <StatusBadge status={token.status} />
                          </td>
                          <td>{formatExpiry(token.expiresAt)}</td>
                          <td>{formatUnixMillis(token.createdAt)}</td>
                          <td>{formatLastUsed(token.lastUsedAt)}</td>
                          <td
                            className="access-control__actions"
                            onClick={(event) => event.stopPropagation()}
                          >
                            {token.status === "active" ? (
                              <button
                                type="button"
                                className="btn btn--ghost btn--small"
                                onClick={() => void handleChangeStatus(token, "disabled")}
                                disabled={actionBusy}
                                aria-label={`禁用 ${token.name}`}
                              >
                                禁用
                              </button>
                            ) : null}
                            {token.status === "disabled" ? (
                              <button
                                type="button"
                                className="btn btn--ghost btn--small"
                                onClick={() => void handleChangeStatus(token, "active")}
                                disabled={actionBusy}
                                aria-label={`启用 ${token.name}`}
                              >
                                启用
                              </button>
                            ) : null}
                            {token.status !== "revoked" ? (
                              <button
                                type="button"
                                className="btn btn--ghost btn--small"
                                onClick={() => setPendingAction({ kind: "revoke", token })}
                                disabled={actionBusy}
                                aria-label={`撤销 ${token.name}`}
                              >
                                撤销
                              </button>
                            ) : null}
                            <button
                              type="button"
                              className="btn btn--ghost btn--small access-control__danger"
                              onClick={() => setPendingAction({ kind: "delete", token })}
                              disabled={token.status === "active" || actionBusy}
                              title={
                                token.status === "active"
                                  ? "启用中的 Token 不能删除，请先禁用或撤销"
                                  : undefined
                              }
                              aria-label={`删除 ${token.name}`}
                            >
                              删除
                            </button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            ) : null}
          </SectionCard>
        </div>

        {selectedToken ? (
          <TokenDetailPanel
            key={selectedToken.id}
            token={selectedToken}
            onClose={() => setSelectedToken(null)}
          />
        ) : null}
      </div>

      {createOpen ? (
        <CreateTokenDialog
          busy={createBusy}
          error={createError}
          onCancel={() => {
            if (!createBusy) {
              setCreateOpen(false);
            }
          }}
          onSubmit={(input) => void handleCreate(input)}
        />
      ) : null}

      {revealed ? (
        <TokenRevealDialog
          token={revealed.token}
          plaintext={revealed.plaintext}
          onClose={handleCloseReveal}
        />
      ) : null}

      <ConfirmDialog
        open={pendingAction !== null}
        title={pendingAction?.kind === "revoke" ? "撤销 Token" : "删除 Token"}
        description={
          pendingAction?.kind === "revoke" ? (
            <div>
              <p>撤销后不可恢复，并会立即终止其活动会话。</p>
              <p>Token：{pendingAction.token.name}</p>
            </div>
          ) : pendingAction ? (
            <div>
              <p>删除后无法恢复，该 Token 及其授权记录将被移除。</p>
              <p>Token：{pendingAction.token.name}</p>
            </div>
          ) : null
        }
        confirmText={pendingAction?.kind === "revoke" ? "确认撤销" : "确认删除"}
        tone="danger"
        busy={actionBusy}
        onConfirm={() => {
          if (pendingAction) {
            void handleConfirmAction(pendingAction);
          }
        }}
        onCancel={() => {
          if (!actionBusy) {
            setPendingAction(null);
          }
        }}
      />
    </section>
  );
}
