import { useEffect, useState } from "react";
import {
  deleteConnection,
  duplicateConnection,
  getConnection,
  listConnections,
  listProtocols,
  setConnectionEnabled,
  testConnection,
} from "../../api/client";
import type {
  ConnectionDetailDto,
  ConnectionDto,
  ConnectionFilterDto,
  ConnectionTestResult,
} from "../../api/types";
import ConfirmDialog from "../../components/ConfirmDialog";
import EmptyState from "../../components/EmptyState";
import SectionCard from "../../components/SectionCard";
import Toolbar from "../../components/Toolbar";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import ConnectionDetailPanel from "./ConnectionDetailPanel";
import ConnectionFormDialog from "./ConnectionFormDialog";
import "./connections.css";

const PAGE_SIZE = 20;
const SEARCH_DEBOUNCE_MS = 300;

type EnabledFilter = "all" | "enabled" | "disabled";
type DialogState = { mode: "create" } | { mode: "edit"; id: string };

const alertStyle = {
  margin: "0 0 12px",
  padding: "10px 12px",
  fontSize: 13,
  color: "var(--color-danger)",
  backgroundColor: "var(--color-danger-soft)",
  borderRadius: "var(--radius)",
};

const noticeStyle = {
  margin: "0 0 12px",
  padding: "10px 12px",
  fontSize: 13,
  color: "var(--color-accent)",
  backgroundColor: "var(--color-accent-soft)",
  borderRadius: "var(--radius)",
};

const controlStyle = { width: "auto", minWidth: 120 };

function TableSkeleton() {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "4px 0" }}>
      <p role="status" style={{ margin: 0, fontSize: 13, color: "var(--color-muted)" }}>
        正在加载连接列表…
      </p>
      <div aria-hidden="true" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {[0, 1, 2, 3].map((row) => (
          <div
            key={row}
            style={{
              height: 18,
              borderRadius: 4,
              backgroundColor: "var(--color-border)",
              opacity: 1 - row * 0.18,
            }}
          />
        ))}
      </div>
    </div>
  );
}

export default function ConnectionsPage() {
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [protocolId, setProtocolId] = useState("");
  const [enabledFilter, setEnabledFilter] = useState<EnabledFilter>("all");
  const [offset, setOffset] = useState(0);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<ConnectionDto | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [duplicating, setDuplicating] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setSearch(searchInput);
      setOffset(0);
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [searchInput]);

  const protocols = useAsync(() => listProtocols(), []);

  const filter: ConnectionFilterDto = {
    offset,
    limit: PAGE_SIZE,
    protocolId: protocolId === "" ? null : protocolId,
    enabled: enabledFilter === "all" ? null : enabledFilter === "enabled",
    search: search.trim() === "" ? null : search.trim(),
  };

  const list = useAsync(() => listConnections(filter), [
    search,
    protocolId,
    enabledFilter,
    offset,
  ]);

  const detail = useAsync<ConnectionDetailDto | null>(
    () => (selectedId ? getConnection(selectedId) : Promise.resolve(null)),
    [selectedId],
  );

  const total = list.data?.total ?? 0;
  const items = list.data?.items ?? [];
  const rangeStart = total === 0 ? 0 : offset + 1;
  const rangeEnd = offset + items.length;
  const canPrev = offset > 0;
  const canNext = offset + PAGE_SIZE < total;
  const hasFilters = search.trim() !== "" || protocolId !== "" || enabledFilter !== "all";

  const handleRefresh = () => {
    setActionError(null);
    list.reload();
    if (selectedId) {
      detail.reload();
    }
  };

  const handleSelect = (id: string) => {
    setSelectedId(id);
    setTestResult(null);
    setTestError(null);
  };

  const handleClearFilters = () => {
    setSearchInput("");
    setSearch("");
    setProtocolId("");
    setEnabledFilter("all");
    setOffset(0);
  };

  const handleToggleEnabled = async (connection: ConnectionDto, enabled: boolean) => {
    setPendingId(connection.id);
    setActionError(null);
    try {
      await setConnectionEnabled(connection.id, enabled);
      setNotice(`已${enabled ? "启用" : "停用"}连接「${connection.name}」`);
      list.reload();
      if (selectedId === connection.id) {
        detail.reload();
      }
    } catch (error) {
      setActionError(toUserMessage(error));
    } finally {
      setPendingId(null);
    }
  };

  const handleTestSelected = async () => {
    if (!selectedId) {
      return;
    }
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    try {
      setTestResult(await testConnection(selectedId));
    } catch (error) {
      setTestError(toUserMessage(error));
    } finally {
      setTesting(false);
    }
  };

  const handleDuplicate = async () => {
    if (!selectedId) {
      return;
    }
    setDuplicating(true);
    setActionError(null);
    try {
      const copy = await duplicateConnection(selectedId);
      setNotice(`已复制为「${copy.name}」`);
      setSelectedId(copy.id);
      setTestResult(null);
      setTestError(null);
      list.reload();
    } catch (error) {
      setActionError(toUserMessage(error));
    } finally {
      setDuplicating(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (!confirmDelete) {
      return;
    }
    setDeleting(true);
    try {
      await deleteConnection(confirmDelete.id);
      setNotice(`已删除连接「${confirmDelete.name}」`);
      setConfirmDelete(null);
      if (selectedId === confirmDelete.id) {
        setSelectedId(null);
        setTestResult(null);
        setTestError(null);
      }
      if (items.length === 1 && offset > 0) {
        setOffset(Math.max(0, offset - PAGE_SIZE));
      } else {
        list.reload();
      }
    } catch (error) {
      setActionError(toUserMessage(error));
      setConfirmDelete(null);
    } finally {
      setDeleting(false);
    }
  };

  const handleSaved = (connection: ConnectionDto) => {
    const alreadySelected = selectedId === connection.id;
    setDialog(null);
    setSelectedId(connection.id);
    setNotice(`已保存连接「${connection.name}」`);
    setTestResult(null);
    setTestError(null);
    list.reload();
    if (alreadySelected) {
      detail.reload();
    }
  };

  const detailProtocol =
    protocols.data?.find(
      (item) => item.protocolId === detail.data?.connection.protocolId,
    ) ?? null;

  return (
    <section className="page connections-page">
      <header className="connections-page__header">
        <h1>连接管理</h1>
        <p>
          创建并维护 MCP 服务连接。配置项由协议动态提供，测试通过后再授权给 Token 使用。
        </p>
      </header>

      <Toolbar
        actions={
          <>
            <button
              type="button"
              className="btn btn--ghost"
              onClick={handleRefresh}
              disabled={list.loading}
            >
              刷新
            </button>
            <button
              type="button"
              className="btn btn--primary"
              onClick={() => setDialog({ mode: "create" })}
            >
              新建连接
            </button>
          </>
        }
      >
        <input
          type="search"
          aria-label="搜索连接"
          className="form-field__control connections-filters__search"
          placeholder="搜索名称、描述或标签"
          value={searchInput}
          onChange={(event) => setSearchInput(event.target.value)}
        />
        <select
          aria-label="按协议筛选"
          className="form-field__control"
          value={protocolId}
          onChange={(event) => {
            setProtocolId(event.target.value);
            setOffset(0);
          }}
          style={controlStyle}
        >
          <option value="">全部协议</option>
          {(protocols.data ?? []).map((item) => (
            <option key={item.protocolId} value={item.protocolId}>
              {item.displayName}
            </option>
          ))}
        </select>
        <select
          aria-label="按启用状态筛选"
          className="form-field__control"
          value={enabledFilter}
          onChange={(event) => {
            setEnabledFilter(event.target.value as EnabledFilter);
            setOffset(0);
          }}
          style={controlStyle}
        >
          <option value="all">全部状态</option>
          <option value="enabled">仅已启用</option>
          <option value="disabled">仅已停用</option>
        </select>
      </Toolbar>

      {protocols.error ? (
        <p role="alert" style={alertStyle}>
          协议列表加载失败：{toUserMessage(protocols.error)}
        </p>
      ) : null}
      {actionError ? (
        <p role="alert" style={alertStyle}>
          {actionError}
        </p>
      ) : null}
      {notice ? (
        <p role="status" style={noticeStyle}>
          {notice}
        </p>
      ) : null}

      <div
        className={`connections-layout${
          selectedId ? " connections-layout--with-detail" : ""
        }`}
      >
        <div className="connections-layout__main">
          <SectionCard
            title="连接列表"
            description="列表仅展示连接的公共信息，凭据不会在此显示。"
            actions={
              <span style={{ fontSize: 12, color: "var(--color-muted)" }}>
                共 {total} 条
              </span>
            }
          >
            {list.error ? (
              <p role="alert" style={alertStyle}>
                {toUserMessage(list.error)}
              </p>
            ) : null}

            {!list.data && list.loading ? <TableSkeleton /> : null}

            {list.data && items.length === 0 && !list.loading && !list.error ? (
              <EmptyState
                title={hasFilters ? "没有匹配的连接" : "暂无连接"}
                description={
                  hasFilters
                    ? "尝试调整搜索关键词或筛选条件。"
                    : "创建第一个连接后即可在此测试、启停与查看授权提示。"
                }
                action={
                  hasFilters ? (
                    <button
                      type="button"
                      className="btn btn--ghost"
                      onClick={handleClearFilters}
                    >
                      清空筛选
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn btn--primary"
                      onClick={() => setDialog({ mode: "create" })}
                    >
                      创建连接
                    </button>
                  )
                }
              />
            ) : null}

            {items.length > 0 ? (
              <>
                <div className="connections-table-wrap" aria-busy={list.loading}>
                  <table className="connections-table">
                    <thead>
                      <tr>
                        {["名称", "协议", "能力", "启用", "更新时间", "操作"].map(
                          (label) => (
                            <th key={label} scope="col">
                              {label}
                            </th>
                          ),
                        )}
                      </tr>
                    </thead>
                    <tbody>
                      {items.map((connection) => (
                        <tr key={connection.id}>
                          <td>
                            <button
                              type="button"
                              className="connections-table__name"
                              onClick={() => handleSelect(connection.id)}
                            >
                              {connection.name}
                            </button>
                            {connection.description ? (
                              <div className="connections-table__description">
                                {connection.description}
                              </div>
                            ) : null}
                          </td>
                          <td>{connection.protocolDisplayName}</td>
                          <td>
                            <div
                              className="connections-table__capabilities"
                              title={connection.capabilities.join("、")}
                            >
                              {connection.capabilities.length === 0
                                ? "—"
                                : connection.capabilities.join("、")}
                            </div>
                          </td>
                          <td>
                            <input
                              type="checkbox"
                              role="switch"
                              aria-label={`启用 ${connection.name}`}
                              checked={connection.enabled}
                              disabled={pendingId === connection.id}
                              onChange={(event) =>
                                void handleToggleEnabled(connection, event.target.checked)
                              }
                              className="connections-table__switch"
                            />
                          </td>
                          <td className="connections-table__updated">
                            {formatUnixMillis(connection.updatedAt)}
                          </td>
                          <td>
                            <div className="connections-table__actions">
                              <button
                                type="button"
                                className="btn btn--ghost btn--small"
                                onClick={() => handleSelect(connection.id)}
                              >
                                详情
                              </button>
                              <button
                                type="button"
                                className="btn btn--ghost btn--small"
                                onClick={() => setDialog({ mode: "edit", id: connection.id })}
                              >
                                编辑
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>

                <div className="connections-pagination">
                  <span>
                    第 {rangeStart}-{rangeEnd} 条，共 {total} 条
                  </span>
                  <div className="connections-pagination__actions">
                    <button
                      type="button"
                      className="btn btn--ghost btn--small"
                      onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
                      disabled={!canPrev}
                    >
                      上一页
                    </button>
                    <button
                      type="button"
                      className="btn btn--ghost btn--small"
                      onClick={() => setOffset(offset + PAGE_SIZE)}
                      disabled={!canNext}
                    >
                      下一页
                    </button>
                  </div>
                </div>
              </>
            ) : null}
          </SectionCard>
        </div>

        {selectedId ? (
          <div className="connections-layout__aside">
            <ConnectionDetailPanel
              detail={detail.data}
              loading={detail.loading}
              error={detail.error}
              protocol={detailProtocol}
              busy={duplicating}
              testing={testing}
              testResult={testResult}
              testError={testError}
              onClose={() => {
                setSelectedId(null);
                setTestResult(null);
                setTestError(null);
              }}
              onEdit={() => setDialog({ mode: "edit", id: selectedId })}
              onDelete={() => {
                if (detail.data) {
                  setConfirmDelete(detail.data.connection);
                }
              }}
              onDuplicate={() => void handleDuplicate()}
              onTest={() => void handleTestSelected()}
            />
          </div>
        ) : null}
      </div>

      {dialog ? (
        <ConnectionFormDialog
          mode={dialog.mode}
          connectionId={dialog.mode === "edit" ? dialog.id : undefined}
          protocols={protocols.data ?? []}
          onClose={() => setDialog(null)}
          onSaved={handleSaved}
        />
      ) : null}

      <ConfirmDialog
        open={confirmDelete !== null}
        title="删除连接"
        description={
          confirmDelete
            ? `确定删除连接「${confirmDelete.name}」吗？删除后无法恢复。`
            : undefined
        }
        confirmText="删除"
        tone="danger"
        busy={deleting}
        onConfirm={() => void handleConfirmDelete()}
        onCancel={() => {
          if (!deleting) {
            setConfirmDelete(null);
          }
        }}
      />
    </section>
  );
}
