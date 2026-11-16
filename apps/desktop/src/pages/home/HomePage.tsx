import { useMemo, useState } from "react";
import type { FormEvent } from "react";
import {
  createToken,
  getOverview,
  getTokenMcpConfig,
  listTokens,
  startMcp,
  stopMcp,
} from "../../api/client";
import type {
  CreatedTokenDto,
  McpStatusDto,
  TokenDto,
} from "../../api/types";
import EmptyState from "../../components/EmptyState";
import SectionCard from "../../components/SectionCard";
import StatusBadge from "../../components/StatusBadge";
import { formatUnixMillis, toUserMessage } from "../../lib/errors";
import { useAsync } from "../../lib/useAsync";
import { auditKindLabel, auditOutcomeLabel, summarizeCommand } from "../audit/audit";
import {
  buildAiPrompt,
} from "./home";
import "./home.css";

const TOKEN_PAGE_SIZE = 50;
const RECENT_AUDIT_LIMIT = 5;
const NEW_TOKEN_OPTION = "__new__";

type Feedback = {
  tone: "success" | "error";
  message: string;
};

function StatCard({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="home__stat">
      <span className="home__stat-label">{label}</span>
      <strong className="home__stat-value">{value === null ? "—" : value}</strong>
    </div>
  );
}

export default function HomePage() {
  const overviewQuery = useAsync(() => getOverview(), []);
  const tokensQuery = useAsync(() => listTokens({ offset: 0, limit: TOKEN_PAGE_SIZE }), []);

  const [mcpOverride, setMcpOverride] = useState<McpStatusDto | null>(null);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);

  const [created, setCreated] = useState<CreatedTokenDto | null>(null);
  const [tokenName, setTokenName] = useState("");
  const [tokenNameError, setTokenNameError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(null);
  const [promptCopied, setPromptCopied] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);

  const overview = overviewQuery.data;
  const tokens = useMemo(() => tokensQuery.data?.items ?? [], [tokensQuery.data]);
  const mcp = mcpOverride ?? overview?.mcp ?? null;

  const tokenOptions = useMemo(() => {
    const base = created ? [created.token, ...tokens] : tokens;
    const seen = new Set<string>();
    return base.filter((token) => {
      if (seen.has(token.id)) {
        return false;
      }
      seen.add(token.id);
      return true;
    });
  }, [created, tokens]);

  const creatingToken = selectedTokenId === NEW_TOKEN_OPTION || tokenOptions.length === 0;

  const selectedToken: TokenDto | null = useMemo(() => {
    if (creatingToken) {
      return null;
    }
    return (
      tokenOptions.find((token) => token.id === selectedTokenId) ??
      tokenOptions[0] ??
      null
    );
  }, [creatingToken, tokenOptions, selectedTokenId]);

  const recentAudits = overview?.recentAudits.slice(0, RECENT_AUDIT_LIMIT) ?? [];

  const configQuery = useAsync(
    () =>
      selectedToken
        ? getTokenMcpConfig(selectedToken.id)
        : Promise.resolve(""),
    [selectedToken?.id ?? ""],
  );
  const configText = configQuery.loading ? "" : (configQuery.data ?? "");
  const configError = configQuery.error ? toUserMessage(configQuery.error) : null;
  const promptText = useMemo(
    () => (configText ? buildAiPrompt(configText) : ""),
    [configText],
  );

  const step1Done = mcp?.running === true;
  const step2Done = selectedToken !== null;
  const step3Done = promptCopied;
  const completedSteps = [step1Done, step2Done, step3Done].filter(Boolean).length;

  const copyText = async (text: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setFeedback({ tone: "success", message: successMessage });
      return true;
    } catch {
      setFeedback({ tone: "error", message: "复制失败，请手动选中内容后复制。" });
      return false;
    }
  };

  const handleToggleMcp = async () => {
    if (!mcp || mcpBusy) {
      return;
    }
    setMcpBusy(true);
    setMcpError(null);
    try {
      const status = mcp.running ? await stopMcp() : await startMcp();
      setMcpOverride(status);
      overviewQuery.reload();
    } catch (error) {
      setMcpError(toUserMessage(error));
    } finally {
      setMcpBusy(false);
    }
  };

  const handleCreateToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = tokenName.trim();
    if (name === "") {
      setTokenNameError("Token 名称为必填项");
      return;
    }
    setTokenNameError(null);
    setCreateError(null);
    setCreateBusy(true);
    try {
      const result = await createToken({
        name,
        permission: "read_only",
        allowInteractive: false,
      });
      setCreated(result);
      setSelectedTokenId(result.token.id);
      setTokenName("");
      setPromptCopied(false);
      overviewQuery.reload();
      tokensQuery.reload();
    } catch (error) {
      setCreateError(toUserMessage(error));
    } finally {
      setCreateBusy(false);
    }
  };

  const handleSelectToken = (tokenId: string) => {
    setSelectedTokenId(tokenId);
    setPromptCopied(false);
  };

  const handleCopyPrompt = async () => {
    if (!promptText) {
      return;
    }
    const ok = await copyText(promptText, "AI 接入提示词已复制到剪贴板。");
    if (ok) {
      setPromptCopied(true);
    }
  };

  const startDisabled = mcp === null || mcp.running || mcpBusy;
  const stopDisabled = mcp === null || !mcp.running || mcpBusy;

  return (
    <section className="page home">
      <header className="home__header">
        <div>
          <h1>首页</h1>
          <p>查看运行概览，并按 3 个步骤把 MCP Server 接入其他 AI 客户端。</p>
        </div>
        <span className="home__progress">已完成 {completedSteps} / 3 步</span>
      </header>

      <p
        className={`home__feedback${
          feedback ? ` home__feedback--${feedback.tone}` : ""
        }`}
        role="status"
        aria-live="polite"
      >
        {feedback?.message ?? ""}
      </p>

      {overviewQuery.error ? (
        <div className="home__alert home__alert--danger" role="alert">
          <span>概览加载失败：{toUserMessage(overviewQuery.error)}</span>
          <button
            type="button"
            className="btn btn--ghost btn--small"
            onClick={overviewQuery.reload}
            disabled={overviewQuery.loading}
          >
            重试
          </button>
        </div>
      ) : null}

      <div className="home__stats">
        <StatCard label="连接总数" value={overview ? overview.connectionCount : null} />
        <StatCard
          label="已启用连接"
          value={overview ? overview.enabledConnectionCount : null}
        />
        <StatCard label="Token 数" value={overview ? overview.tokenCount : null} />
        <StatCard
          label="活跃会话"
          value={overview ? overview.activeSessionCount : null}
        />
      </div>

      <SectionCard
        title="MCP 服务"
        description="内置 Streamable HTTP MCP Server，默认仅监听本机地址。"
      >
        <dl className="home__meta">
          <div className="home__meta-item">
            <dt>服务地址</dt>
            <dd>
              <code>{mcp?.endpoint ?? "—"}</code>
            </dd>
          </div>
          <div className="home__meta-item">
            <dt>监听地址</dt>
            <dd>{mcp ? `${mcp.host}:${mcp.port}` : "—"}</dd>
          </div>
          <div className="home__meta-item">
            <dt>端口</dt>
            <dd>{mcp?.port ?? "—"}</dd>
          </div>
          <div className="home__meta-item">
            <dt>运行状态</dt>
            <dd>
              <StatusBadge
                status={mcp?.running ? "running" : "disabled"}
                label={mcp?.running ? "运行中" : "未运行"}
              />
            </dd>
          </div>
        </dl>

        {mcp?.lastError ? (
          <p className="home__alert home__alert--danger" role="alert">
            服务异常：{mcp.lastError}
          </p>
        ) : null}
        {mcpError ? (
          <p className="home__alert home__alert--danger" role="alert">
            {mcpError}
          </p>
        ) : null}

        <div className="home__actions">
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void handleToggleMcp()}
            disabled={startDisabled}
          >
            {mcpBusy && !mcp?.running ? "启动中…" : "启动服务"}
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => void handleToggleMcp()}
            disabled={stopDisabled}
          >
            {mcpBusy && mcp?.running ? "停止中…" : "停止服务"}
          </button>
        </div>
      </SectionCard>

      <SectionCard
        title="快速开始"
        description="按 3 个步骤完成接入，复制提示词后交给 AI 客户端即可。"
      >
        <ol className="quickstart">
          <li className="quickstart__step">
            <div className="quickstart__head">
              <span className="quickstart__index" aria-hidden="true">
                1
              </span>
              <h3>启动 MCP</h3>
              <StatusBadge
                status={step1Done ? "succeeded" : "disabled"}
                label={step1Done ? "已完成" : "待完成"}
              />
            </div>
            <p className="quickstart__text">
              {mcp?.endpoint ? (
                <code>{mcp.endpoint}</code>
              ) : (
                "启动后显示服务地址"
              )}
            </p>
            <div className="home__actions">
              {mcp?.running ? (
                <button
                  type="button"
                  className="btn btn--ghost btn--small"
                  onClick={() => void handleToggleMcp()}
                  disabled={stopDisabled}
                >
                  停止 MCP
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn--primary btn--small"
                  onClick={() => void handleToggleMcp()}
                  disabled={startDisabled}
                >
                  启动 MCP
                </button>
              )}
            </div>
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <span className="quickstart__index" aria-hidden="true">
                2
              </span>
              <h3>选择访问 Token</h3>
              <StatusBadge
                status={step2Done ? "succeeded" : "disabled"}
                label={step2Done ? "已完成" : "待完成"}
              />
            </div>

            <div className="form-field">
              <label className="form-field__label" htmlFor="home-token-select">
                访问 Token
              </label>
              <select
                id="home-token-select"
                className="form-field__control"
                value={
                  creatingToken
                    ? NEW_TOKEN_OPTION
                    : (selectedToken?.id ?? NEW_TOKEN_OPTION)
                }
                onChange={(event) => handleSelectToken(event.target.value)}
              >
                {tokenOptions.map((token) => (
                  <option key={token.id} value={token.id}>
                    {token.name}
                  </option>
                ))}
                <option value={NEW_TOKEN_OPTION}>＋ 新建 Token</option>
              </select>
            </div>

            {creatingToken ? (
              <form
                className="quickstart__create"
                onSubmit={(event) => void handleCreateToken(event)}
              >
                <div className="form-field">
                  <label
                    className="form-field__label form-field__label--required"
                    htmlFor="home-token-name"
                  >
                    Token 名称
                  </label>
                  <input
                    id="home-token-name"
                    className="form-field__control"
                    value={tokenName}
                    placeholder="例如：Claude Code"
                    onChange={(event) => {
                      setTokenName(event.target.value);
                      if (tokenNameError) {
                        setTokenNameError(null);
                      }
                    }}
                  />
                  {tokenNameError ? (
                    <p className="form-field__error" role="alert">
                      {tokenNameError}
                    </p>
                  ) : null}
                </div>
                <div className="home__actions">
                  <button
                    type="submit"
                    className="btn btn--primary btn--small"
                    disabled={createBusy}
                  >
                    {createBusy ? "创建中…" : "创建 Token"}
                  </button>
                </div>
                {createError ? (
                  <p className="home__alert home__alert--danger" role="alert">
                    {createError}
                  </p>
                ) : null}
              </form>
            ) : (
              <p className="quickstart__text">配置与提示词将使用该 Token。</p>
            )}

            {!creatingToken && configError ? (
              <p className="home__alert home__alert--danger" role="alert">
                {configError}
              </p>
            ) : null}
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <span className="quickstart__index" aria-hidden="true">
                3
              </span>
              <h3>复制提示词</h3>
              <StatusBadge
                status={step3Done ? "succeeded" : "disabled"}
                label={step3Done ? "已完成" : "待完成"}
              />
            </div>
            <p className="quickstart__text">
              已包含所选 Token 的 MCP 配置，AI 接入后会自行探索可用工具。
            </p>
            <div className="home__actions">
              <button
                type="button"
                className="btn btn--primary btn--small"
                onClick={() => void handleCopyPrompt()}
                disabled={promptText === ""}
              >
                复制接入提示词
              </button>
            </div>
          </li>
        </ol>
      </SectionCard>

      <SectionCard title="最近操作" description="最近 5 条审计记录，只读展示。">
        {overviewQuery.loading && overview === null ? (
          <p className="quickstart__text">正在加载最近操作…</p>
        ) : null}
        {!overviewQuery.loading && recentAudits.length === 0 ? (
          <EmptyState
            title="暂无审计记录"
            description="MCP 客户端开始操作后，这里会展示最近的审计事件。"
          />
        ) : null}
        {recentAudits.length > 0 ? (
          <ul className="home__audit-list">
            {recentAudits.map((audit) => (
              <li className="home__audit-item" key={audit.id}>
                <span className="home__audit-kind">{auditKindLabel(audit.kind)}</span>
                <StatusBadge
                  status={audit.outcome}
                  label={auditOutcomeLabel(audit.outcome)}
                />
                <code className="home__audit-command">
                  {summarizeCommand(audit.command)}
                </code>
                <time className="home__audit-time">
                  {formatUnixMillis(audit.startedAt)}
                </time>
              </li>
            ))}
          </ul>
        ) : null}
      </SectionCard>
    </section>
  );
}
