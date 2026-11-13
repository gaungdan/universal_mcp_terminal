import { useState } from "react";
import type { FormEvent } from "react";
import {
  deleteHostKey,
  getSettings,
  listHostKeys,
  updateMcpSettings,
  updateSettings,
} from "../../api/client";
import type { McpStatusDto, SettingsDto } from "../../api/types";
import SectionCard from "../../components/SectionCard";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import "./settings.css";

const EXECUTION_MODES = [
  { value: "ReadOnly", label: "ReadOnly（只读）" },
  { value: "Restricted", label: "Restricted（受限）" },
  { value: "FullAccess", label: "FullAccess（完全访问）" },
];

const LOG_LEVELS = ["info", "debug", "warn", "error"];

const SECURITY_HINTS: Record<string, string> = {
  ReadOnly: "只允许只读类操作，命令执行与写入会被拒绝。",
  Restricted: "允许常规操作，高风险操作仍需用户确认。",
  FullAccess: "允许全部操作，请仅在受信任环境中使用。",
};

type SettingsFormProps = {
  initial: SettingsDto;
};

function SettingsForm({ initial }: SettingsFormProps) {
  const [host, setHost] = useState(initial.mcp.host);
  const [port, setPort] = useState(String(initial.mcp.port));
  const [mcpStatus, setMcpStatus] = useState<McpStatusDto>(initial.mcp);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);
  const [mcpNotice, setMcpNotice] = useState<string | null>(null);

  const [executionMode, setExecutionMode] = useState(
    initial.security.defaultExecutionMode,
  );
  const [timeoutSecs, setTimeoutSecs] = useState(
    String(initial.security.sessionTimeoutSecs),
  );
  const [requireConfirmation, setRequireConfirmation] = useState(
    initial.security.requireConfirmationForHighRisk,
  );
  const [logLevel, setLogLevel] = useState(initial.logLevel);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [settingsNotice, setSettingsNotice] = useState<string | null>(null);

  const hostError = host.trim() === "" ? "监听地址为必填项" : null;
  const portValue = Number(port);
  const portError =
    port.trim() === ""
      ? "端口为必填项"
      : !Number.isInteger(portValue) || portValue < 1 || portValue > 65535
        ? "端口必须是 1-65535 之间的整数"
        : null;
  const timeoutValue = Number(timeoutSecs);
  const timeoutError =
    timeoutSecs.trim() === ""
      ? "Session 超时时间为必填项"
      : !Number.isInteger(timeoutValue) || timeoutValue <= 0
        ? "Session 超时时间必须是大于 0 的整数"
        : null;

  const handleSaveMcp = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (hostError || portError) {
      return;
    }
    setMcpBusy(true);
    setMcpError(null);
    setMcpNotice(null);
    try {
      const status = await updateMcpSettings({ host: host.trim(), port: portValue });
      setMcpStatus(status);
      setMcpNotice(
        status.running
          ? "MCP 配置已保存，正在运行的服务需重启后生效。"
          : "MCP 配置已保存。",
      );
    } catch (error) {
      setMcpError(toUserMessage(error));
    } finally {
      setMcpBusy(false);
    }
  };

  const handleSaveSecurity = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (timeoutError) {
      return;
    }
    setSettingsBusy(true);
    setSettingsError(null);
    setSettingsNotice(null);
    try {
      const result = await updateSettings({
        security: {
          defaultExecutionMode: executionMode,
          sessionTimeoutSecs: timeoutValue,
          requireConfirmationForHighRisk: requireConfirmation,
        },
        logLevel,
      });
      setExecutionMode(result.security.defaultExecutionMode);
      setTimeoutSecs(String(result.security.sessionTimeoutSecs));
      setRequireConfirmation(result.security.requireConfirmationForHighRisk);
      setLogLevel(result.logLevel);
      setMcpStatus(result.mcp);
      setSettingsNotice("安全设置已保存。");
    } catch (error) {
      setSettingsError(toUserMessage(error));
    } finally {
      setSettingsBusy(false);
    }
  };

  return (
    <div className="settings__layout">
      <SectionCard
        title="MCP Server 配置"
        description="默认只监听 127.0.0.1，修改后保存即可写入配置。"
      >
        <form className="settings__form" onSubmit={(event) => void handleSaveMcp(event)}>
          <div className="settings__grid">
            <div className={`form-field${hostError ? " form-field--invalid" : ""}`}>
              <label
                className="form-field__label form-field__label--required"
                htmlFor="settings-host"
              >
                监听地址
              </label>
              <input
                id="settings-host"
                className="form-field__control"
                value={host}
                placeholder="127.0.0.1"
                onChange={(event) => setHost(event.target.value)}
              />
              {hostError ? (
                <p className="form-field__error" role="alert">
                  {hostError}
                </p>
              ) : null}
            </div>

            <div className={`form-field${portError ? " form-field--invalid" : ""}`}>
              <label
                className="form-field__label form-field__label--required"
                htmlFor="settings-port"
              >
                端口
              </label>
              <input
                id="settings-port"
                className="form-field__control"
                type="number"
                inputMode="numeric"
                min={1}
                max={65535}
                value={port}
                onChange={(event) => setPort(event.target.value)}
              />
              {portError ? (
                <p className="form-field__error" role="alert">
                  {portError}
                </p>
              ) : null}
            </div>
          </div>

          {mcpStatus.running ? (
            <p className="settings__note">
              MCP 服务正在运行，修改监听地址或端口后需重启服务才能生效。
            </p>
          ) : null}

          {mcpError ? (
            <p className="settings__alert settings__alert--danger" role="alert">
              {mcpError}
            </p>
          ) : null}
          {mcpNotice ? (
            <p className="settings__alert settings__alert--success" role="status">
              {mcpNotice}
            </p>
          ) : null}

          <div className="settings__actions">
            <button type="submit" className="btn btn--primary" disabled={mcpBusy}>
              {mcpBusy ? "保存中…" : "保存 MCP 配置"}
            </button>
          </div>
        </form>
      </SectionCard>

      <SectionCard
        title="安全与日志"
        description="控制默认执行模式、Session 超时与日志级别。"
      >
        <form
          className="settings__form"
          onSubmit={(event) => void handleSaveSecurity(event)}
        >
          <div className="settings__grid">
            <div className="form-field">
              <label className="form-field__label" htmlFor="settings-execution-mode">
                默认执行模式
              </label>
              <select
                id="settings-execution-mode"
                className="form-field__control"
                value={executionMode}
                onChange={(event) => setExecutionMode(event.target.value)}
              >
                {EXECUTION_MODES.map((mode) => (
                  <option key={mode.value} value={mode.value}>
                    {mode.label}
                  </option>
                ))}
              </select>
              <p className="form-field__help">
                {SECURITY_HINTS[executionMode] ?? "未识别的执行模式。"}
              </p>
            </div>

            <div
              className={`form-field${timeoutError ? " form-field--invalid" : ""}`}
            >
              <label
                className="form-field__label form-field__label--required"
                htmlFor="settings-timeout"
              >
                Session 超时（秒）
              </label>
              <input
                id="settings-timeout"
                className="form-field__control"
                type="number"
                inputMode="numeric"
                min={1}
                value={timeoutSecs}
                onChange={(event) => setTimeoutSecs(event.target.value)}
              />
              {timeoutError ? (
                <p className="form-field__error" role="alert">
                  {timeoutError}
                </p>
              ) : null}
            </div>

            <div className="form-field">
              <label className="form-field__label" htmlFor="settings-log-level">
                日志级别
              </label>
              <select
                id="settings-log-level"
                className="form-field__control"
                value={logLevel}
                onChange={(event) => setLogLevel(event.target.value)}
              >
                {LOG_LEVELS.map((level) => (
                  <option key={level} value={level}>
                    {level}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <label className="settings__checkbox" htmlFor="settings-confirm">
            <input
              id="settings-confirm"
              type="checkbox"
              checked={requireConfirmation}
              onChange={(event) => setRequireConfirmation(event.target.checked)}
            />
            <span>高风险操作需二次确认</span>
          </label>

          {settingsError ? (
            <p className="settings__alert settings__alert--danger" role="alert">
              {settingsError}
            </p>
          ) : null}
          {settingsNotice ? (
            <p className="settings__alert settings__alert--success" role="status">
              {settingsNotice}
            </p>
          ) : null}

          <div className="settings__actions">
            <button type="submit" className="btn btn--primary" disabled={settingsBusy}>
              {settingsBusy ? "保存中…" : "保存安全设置"}
            </button>
          </div>
        </form>
      </SectionCard>

      <SectionCard
        title="当前运行状态"
        description="只读展示，启动或停止服务请在首页操作。"
      >
        <dl className="settings__status">
          <div className="settings__status-item">
            <dt>Endpoint</dt>
            <dd>
              <code>{mcpStatus.endpoint}</code>
            </dd>
          </div>
          <div className="settings__status-item">
            <dt>监听地址</dt>
            <dd>
              {mcpStatus.host}:{mcpStatus.port}
            </dd>
          </div>
          <div className="settings__status-item">
            <dt>MCP 状态</dt>
            <dd>
              <StatusBadge
                status={mcpStatus.running ? "running" : "disabled"}
                label={mcpStatus.running ? "运行中" : "未运行"}
              />
            </dd>
          </div>
        </dl>
      </SectionCard>
    </div>
  );
}

function HostKeysSection() {
  const query = useAsync(() => listHostKeys(), []);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const handleDelete = async (endpoint: string) => {
    setBusy(endpoint);
    setNotice(null);
    try {
      const deleted = await deleteHostKey(endpoint);
      setNotice(deleted ? `已撤销 ${endpoint} 的信任` : `${endpoint} 不存在`);
      query.reload();
    } catch (error) {
      setNotice(toUserMessage(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <SectionCard
      title="已信任的 SSH Host Key"
      description="撤销后，该主机下次连接会重新要求人工确认指纹。"
    >
      {query.error ? (
        <p className="settings__alert settings__alert--danger" role="alert">
          {toUserMessage(query.error)}
        </p>
      ) : null}

      {query.loading && !query.data ? (
        <p className="settings__note">正在加载 Host Key…</p>
      ) : null}

      {query.data && query.data.length === 0 ? (
        <p className="settings__note">尚未信任任何主机指纹。</p>
      ) : null}

      {query.data && query.data.length > 0 ? (
        <div className="settings__table" role="table" aria-label="已信任的 Host Key">
          {query.data.map((record) => (
            <div className="settings__table-row" role="row" key={record.endpoint}>
              <span role="cell">
                <strong>{record.endpoint}</strong>
                <br />
                <code style={{ wordBreak: "break-all" }}>{record.fingerprint}</code>
              </span>
              <span role="cell">{formatUnixMillis(record.trustedAt)}</span>
              <button
                type="button"
                className="btn btn--danger btn--small"
                onClick={() => void handleDelete(record.endpoint)}
                disabled={busy === record.endpoint}
              >
                {busy === record.endpoint ? "撤销中…" : "撤销信任"}
              </button>
            </div>
          ))}
        </div>
      ) : null}

      {notice ? (
        <p className="settings__alert settings__alert--success" role="status">
          {notice}
        </p>
      ) : null}
    </SectionCard>
  );
}

export default function SettingsPage() {
  const settingsQuery = useAsync(() => getSettings(), []);

  return (
    <section className="page settings">
      <header className="settings__header">
        <h1>系统设置</h1>
        <p>调整 MCP Server 监听参数、默认安全策略与日志级别。</p>
      </header>

      {settingsQuery.error ? (
        <div className="settings__alert settings__alert--danger" role="alert">
          <span>设置加载失败：{toUserMessage(settingsQuery.error)}</span>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            onClick={settingsQuery.reload}
            disabled={settingsQuery.loading}
          >
            重试
          </button>
        </div>
      ) : null}

      {settingsQuery.data === null && settingsQuery.loading ? (
        <p>正在加载系统设置…</p>
      ) : null}

      {settingsQuery.data ? (
        <SettingsForm
          key={`${settingsQuery.data.mcp.host}:${settingsQuery.data.mcp.port}`}
          initial={settingsQuery.data}
        />
      ) : null}

      <HostKeysSection />
    </section>
  );
}
