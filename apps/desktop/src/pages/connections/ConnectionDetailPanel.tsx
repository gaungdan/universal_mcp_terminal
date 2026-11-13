import type {
  ConnectionDetailDto,
  ConnectionTestResult,
  ProtocolInfoDto,
} from "../../api/types";
import SectionCard from "../../components/SectionCard";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import TestResultCard from "./TestResultCard";
import { formatConfigValue } from "./helpers";

export type ConnectionDetailPanelProps = {
  detail: ConnectionDetailDto | null;
  loading: boolean;
  error: unknown;
  protocol: ProtocolInfoDto | null;
  busy: boolean;
  testing: boolean;
  testResult: ConnectionTestResult | null;
  testError: string | null;
  onClose: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onTest: () => void;
};

const termStyle = { color: "var(--color-muted)", fontSize: 13 };
const valueStyle = { margin: 0, fontSize: 13 };

export default function ConnectionDetailPanel({
  detail,
  loading,
  error,
  protocol,
  busy,
  testing,
  testResult,
  testError,
  onClose,
  onEdit,
  onDelete,
  onDuplicate,
  onTest,
}: ConnectionDetailPanelProps) {
  const connection = detail?.connection ?? null;
  const secretFieldsSet = detail?.secretFieldsSet ?? [];
  const config = detail?.config ?? {};

  const secretLabels = secretFieldsSet.map((key) => {
    const field = protocol?.configSchema.fields.find((item) => item.key === key);
    return field ? field.label : key;
  });

  const configEntries = protocol
    ? protocol.configSchema.fields
        .filter((field) => !field.secret && config[field.key] !== undefined)
        .map((field) => ({
          key: field.key,
          label: field.label,
          value: formatConfigValue(config[field.key]),
        }))
    : Object.entries(config)
        .filter(([key]) => !secretFieldsSet.includes(key))
        .map(([key, value]) => ({ key, label: key, value: formatConfigValue(value) }));

  return (
    <SectionCard
      title={connection ? connection.name : "连接详情"}
      description="查看配置、能力与授权提示，并进行测试、复制或删除操作。"
      actions={
        <button type="button" className="btn btn--ghost btn--small" onClick={onClose}>
          关闭详情
        </button>
      }
    >
      {error ? (
        <p
          role="alert"
          style={{
            margin: 0,
            padding: "10px 12px",
            fontSize: 13,
            color: "var(--color-danger)",
            backgroundColor: "var(--color-danger-soft)",
            borderRadius: "var(--radius)",
          }}
        >
          {toUserMessage(error)}
        </p>
      ) : null}

      {loading && !detail ? (
        <p role="status" style={{ margin: 0, color: "var(--color-muted)" }}>
          正在加载连接详情…
        </p>
      ) : null}

      {connection ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
            <StatusBadge status={connection.enabled ? "enabled" : "disabled"} />
            <span style={{ fontSize: 12, color: "var(--color-muted)" }}>
              {connection.protocolDisplayName}
            </span>
            {connection.tags.map((tag) => (
              <span key={tag} className="status-badge status-badge--info">
                {tag}
              </span>
            ))}
          </div>

          {connection.description ? (
            <p style={{ margin: 0, fontSize: 13 }}>{connection.description}</p>
          ) : null}

          <dl
            style={{
              display: "grid",
              gridTemplateColumns: "max-content minmax(0, 1fr)",
              gap: "8px 16px",
              margin: 0,
            }}
          >
            <dt style={termStyle}>能力</dt>
            <dd style={{ ...valueStyle, display: "flex", flexWrap: "wrap", gap: 4 }}>
              {connection.capabilities.length === 0 ? (
                <span style={{ color: "var(--color-muted)" }}>—</span>
              ) : (
                connection.capabilities.map((capability) => (
                  <span key={capability} className="status-badge status-badge--neutral">
                    {capability}
                  </span>
                ))
              )}
            </dd>
            <dt style={termStyle}>凭据</dt>
            <dd style={valueStyle}>
              {secretLabels.length === 0 ? "未保存凭据" : `已保存：${secretLabels.join("、")}`}
            </dd>
            <dt style={termStyle}>创建时间</dt>
            <dd style={valueStyle}>{formatUnixMillis(connection.createdAt)}</dd>
            <dt style={termStyle}>更新时间</dt>
            <dd style={valueStyle}>{formatUnixMillis(connection.updatedAt)}</dd>
            <dt style={termStyle}>备注</dt>
            <dd style={valueStyle}>{connection.notes ?? "—"}</dd>
          </dl>

          <div>
            <h3 style={{ margin: "0 0 8px", fontSize: 13, fontWeight: 600 }}>协议配置</h3>
            {configEntries.length === 0 ? (
              <p style={{ margin: 0, fontSize: 13, color: "var(--color-muted)" }}>
                该连接没有可展示的配置项。
              </p>
            ) : (
              <dl
                style={{
                  display: "grid",
                  gridTemplateColumns: "max-content minmax(0, 1fr)",
                  gap: "6px 16px",
                  margin: 0,
                }}
              >
                {configEntries.map((entry) => (
                  <div key={entry.key} style={{ display: "contents" }}>
                    <dt style={termStyle}>{entry.label}</dt>
                    <dd style={valueStyle}>{entry.value}</dd>
                  </div>
                ))}
              </dl>
            )}
          </div>

          <div
            style={{
              padding: "10px 12px",
              fontSize: 13,
              lineHeight: 1.7,
              color: "var(--color-info)",
              backgroundColor: "var(--color-info-soft)",
              border: "1px solid var(--color-info)",
              borderRadius: "var(--radius)",
            }}
          >
            <p style={{ margin: 0, color: "inherit" }}>
              授权提示：该连接需要在「访问控制」中授权给 Token 后，MCP 客户端才能创建
              Session。
              {connection.enabled
                ? "当前连接已启用。"
                : "当前连接已停用，MCP 不会使用它创建新的 Session。"}
              {secretLabels.length === 0
                ? "当前未保存任何凭据。"
                : "凭据已加密保存，界面不会回显明文。"}
            </p>
          </div>

          <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
            <button
              type="button"
              className="btn btn--primary btn--small"
              onClick={onTest}
              disabled={testing}
            >
              {testing ? "测试中…" : "测试连接"}
            </button>
            <button
              type="button"
              className="btn btn--ghost btn--small"
              onClick={onEdit}
              disabled={busy}
            >
              编辑
            </button>
            <button
              type="button"
              className="btn btn--ghost btn--small"
              onClick={onDuplicate}
              disabled={busy}
            >
              {busy ? "处理中…" : "复制"}
            </button>
            <button
              type="button"
              className="btn btn--danger btn--small"
              onClick={onDelete}
              disabled={busy}
            >
              删除
            </button>
          </div>

          {testError ? (
            <p
              role="alert"
              style={{ margin: 0, fontSize: 13, color: "var(--color-danger)" }}
            >
              {testError}
            </p>
          ) : null}
          {testResult ? <TestResultCard result={testResult} /> : null}
        </div>
      ) : null}
    </SectionCard>
  );
}
