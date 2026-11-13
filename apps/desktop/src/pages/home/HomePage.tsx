import { useMemo, useState } from "react";
import type { FormEvent } from "react";
import {
  createToken,
  getOverview,
  listConnections,
  listTokenGrants,
  listTokens,
  setTokenGrant,
  startMcp,
  stopMcp,
} from "../../api/client";
import type {
  CreatedTokenDto,
  GrantDto,
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
  buildMaskedMcpConfig,
  buildMcpConfig,
  maskTokenPrefix,
} from "./home";
import "./home.css";

const TOKEN_PAGE_SIZE = 50;
const CONNECTION_PAGE_SIZE = 100;
const RECENT_AUDIT_LIMIT = 5;

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

function StepStatus({ done }: { done: boolean }) {
  return (
    <StatusBadge
      status={done ? "succeeded" : "disabled"}
      label={done ? "已完成" : "待完成"}
    />
  );
}

export default function HomePage() {
  const overviewQuery = useAsync(() => getOverview(), []);
  const tokensQuery = useAsync(() => listTokens({ offset: 0, limit: TOKEN_PAGE_SIZE }), []);
  const connectionsQuery = useAsync(
    () =>
      listConnections({
        offset: 0,
        limit: CONNECTION_PAGE_SIZE,
        protocolId: null,
        enabled: null,
        search: null,
      }),
    [],
  );

  const [mcpOverride, setMcpOverride] = useState<McpStatusDto | null>(null);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);

  const [created, setCreated] = useState<CreatedTokenDto | null>(null);
  const [tokenName, setTokenName] = useState("");
  const [tokenNameError, setTokenNameError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [plaintextVisible, setPlaintextVisible] = useState(false);

  const [grantOverrides, setGrantOverrides] = useState<Record<string, GrantDto>>({});
  const [grantError, setGrantError] = useState<string | null>(null);
  const [savingGrantId, setSavingGrantId] = useState<string | null>(null);

  const [pastedToken, setPastedToken] = useState("");
  const [configTokenVisible, setConfigTokenVisible] = useState(false);
  const [configCopied, setConfigCopied] = useState(false);
  const [promptCopied, setPromptCopied] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);

  const overview = overviewQuery.data;
  const tokens = tokensQuery.data?.items ?? [];
  const connections = useMemo(
    () => connectionsQuery.data?.items ?? [],
    [connectionsQuery.data],
  );
  const mcp = mcpOverride ?? overview?.mcp ?? null;

  const currentToken: TokenDto | null =
    created?.token ??
    tokens.find((token) => token.status === "active") ??
    tokens[0] ??
    null;

  const grantsQuery = useAsync(
    () =>
      currentToken
        ? listTokenGrants(currentToken.id)
        : Promise.resolve<GrantDto[]>([]),
    [currentToken?.id ?? ""],
  );

  const grantRows = useMemo(() => {
    const baseGrants = new Map<string, GrantDto>();
    for (const grant of grantsQuery.data ?? []) {
      baseGrants.set(grant.connection.id, grant);
    }
    return connections.map((connection) => {
      const base: GrantDto =
        baseGrants.get(connection.id) ?? {
          connection,
          allowed: false,
          grantedAt: null,
        };
      const overridden = currentToken
        ? grantOverrides[`${currentToken.id}:${connection.id}`]
        : undefined;
      return overridden ?? base;
    });
  }, [connections, grantsQuery.data, grantOverrides, currentToken]);

  const recentAudits = overview?.recentAudits.slice(0, RECENT_AUDIT_LIMIT) ?? [];
  const endpoint = mcp?.endpoint ?? "";
  const tokenSecret = created?.plaintext ?? pastedToken.trim();

  const configText = useMemo(
    () => (endpoint && tokenSecret ? buildMcpConfig(endpoint, tokenSecret) : ""),
    [endpoint, tokenSecret],
  );
  const maskedConfigText = useMemo(() => buildMaskedMcpConfig(endpoint), [endpoint]);
  const promptText = useMemo(
    () => (endpoint ? buildAiPrompt(endpoint) : ""),
    [endpoint],
  );

  const step1Done = mcp?.running === true;
  const step2Done = (overview?.tokenCount ?? 0) > 0 || created !== null;
  const step3Done = grantRows.some((grant) => grant.allowed);
  const step4Done = configCopied;
  const step5Done = promptCopied;
  const completedSteps = [step1Done, step2Done, step3Done, step4Done, step5Done].filter(
    Boolean,
  ).length;

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
      const result = await createToken({ name });
      setCreated(result);
      setTokenName("");
      setPlaintextVisible(false);
      overviewQuery.reload();
      tokensQuery.reload();
    } catch (error) {
      setCreateError(toUserMessage(error));
    } finally {
      setCreateBusy(false);
    }
  };

  const handleToggleGrant = async (grant: GrantDto) => {
    if (!currentToken) {
      return;
    }
    const connectionId = grant.connection.id;
    const nextAllowed = !grant.allowed;
    const key = `${currentToken.id}:${connectionId}`;
    setGrantError(null);
    setGrantOverrides((previous) => ({
      ...previous,
      [key]: {
        ...grant,
        allowed: nextAllowed,
        grantedAt: nextAllowed ? Date.now() : null,
      },
    }));
    setSavingGrantId(connectionId);
    try {
      const updated = await setTokenGrant({
        tokenId: currentToken.id,
        connectionId,
        allowed: nextAllowed,
      });
      setGrantOverrides((previous) => ({ ...previous, [key]: updated }));
    } catch (error) {
      setGrantOverrides((previous) => {
        if (!Object.hasOwn(previous, key)) {
          return previous;
        }
        const next = { ...previous };
        delete next[key];
        return next;
      });
      setGrantError(toUserMessage(error));
    } finally {
      setSavingGrantId(null);
    }
  };

  const handleCopyConfig = async () => {
    if (!configText) {
      return;
    }
    const ok = await copyText(configText, "MCP 配置已复制到剪贴板。");
    if (ok) {
      setConfigCopied(true);
    }
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

  const handleCopyPlaintext = async () => {
    if (!created) {
      return;
    }
    await copyText(created.plaintext, "Token 已复制到剪贴板。");
  };

  const handleRegenerate = () => {
    setConfigCopied(false);
    setConfigTokenVisible(false);
    overviewQuery.reload();
    tokensQuery.reload();
  };

  const startDisabled = mcp === null || mcp.running || mcpBusy;
  const stopDisabled = mcp === null || !mcp.running || mcpBusy;

  return (
    <section className="page home">
      <header className="home__header">
        <div>
          <h1>首页</h1>
          <p>查看运行概览，并按 5 个步骤把 MCP Server 接入其他 AI 客户端。</p>
        </div>
        <span className="home__progress">已完成 {completedSteps} / 5 步</span>
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
        description="步骤状态由当前真实数据推导，完成后即可在 AI 客户端中使用。"
      >
        <ol className="quickstart">
          <li className="quickstart__step">
            <div className="quickstart__head">
              <h3>启动 MCP Server</h3>
              <StepStatus done={step1Done} />
            </div>
            <p className="quickstart__text">
              当前服务地址：
              <code>{mcp?.endpoint ?? "加载中…"}</code>
            </p>
            <div className="home__actions">
              <button
                type="button"
                className="btn btn--primary btn--small"
                onClick={() => void handleToggleMcp()}
                disabled={startDisabled}
              >
                启动 MCP Server
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--small"
                onClick={() => void handleToggleMcp()}
                disabled={stopDisabled}
              >
                停止 MCP Server
              </button>
            </div>
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <h3>创建访问 Token</h3>
              <StepStatus done={step2Done} />
            </div>

            {step2Done && currentToken ? (
              <>
                <p className="quickstart__text">
                  当前 Token：<strong>{currentToken.name}</strong>{" "}
                  <code>{maskTokenPrefix(currentToken.prefix)}</code>
                </p>
                {created ? (
                  <>
                    <div className="home__notice home__notice--warning" role="note">
                      此 Token 明文仅显示一次，请立即复制并妥善保存；离开本页后将无法再次查看。
                    </div>
                    <div className="home__token-row">
                      <code className="home__token-plaintext" data-testid="created-token-plaintext">
                        {plaintextVisible ? created.plaintext : "••••••••••••••••"}
                      </code>
                      <button
                        type="button"
                        className="btn btn--ghost btn--small"
                        aria-pressed={plaintextVisible}
                        onClick={() => setPlaintextVisible((visible) => !visible)}
                      >
                        {plaintextVisible ? "隐藏 Token" : "显示 Token"}
                      </button>
                      <button
                        type="button"
                        className="btn btn--ghost btn--small"
                        onClick={() => void handleCopyPlaintext()}
                      >
                        复制 Token
                      </button>
                    </div>
                  </>
                ) : (
                  <p className="quickstart__text">
                    Token 明文仅在创建时显示一次。如需重新获取明文，请前往{" "}
                    <a href="#/access-control">访问控制</a> 重新创建。
                  </p>
                )}
              </>
            ) : (
              <form className="quickstart__form" onSubmit={(event) => void handleCreateToken(event)}>
                <p className="quickstart__text">
                  还没有访问 Token。创建一个后即可授权连接并生成 MCP 配置。
                  <a href="#/access-control">前往访问控制</a>
                </p>
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
                  <button type="submit" className="btn btn--primary" disabled={createBusy}>
                    {createBusy ? "创建中…" : "创建 Token"}
                  </button>
                </div>
                {createError ? (
                  <p className="home__alert home__alert--danger" role="alert">
                    {createError}
                  </p>
                ) : null}
              </form>
            )}
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <h3>授权连接</h3>
              <StepStatus done={step3Done} />
            </div>

            {!currentToken ? (
              <p className="quickstart__text">请先完成第 2 步，创建可用的访问 Token。</p>
            ) : (
              <>
                <p className="quickstart__text">
                  为 Token「{currentToken.name}」选择允许 AI 使用的连接，开关与访问控制页面保持一致。
                </p>
                {grantError ? (
                  <p className="home__alert home__alert--danger" role="alert">
                    {grantError}
                  </p>
                ) : null}
                {connectionsQuery.error ? (
                  <p className="home__alert home__alert--danger" role="alert">
                    {toUserMessage(connectionsQuery.error)}
                  </p>
                ) : null}
                {connections.length === 0 && !connectionsQuery.loading ? (
                  <EmptyState
                    title="还没有连接"
                    description="先在连接管理中创建连接，再回到这里授权给 Token。"
                    action={<a href="#/connections">前往连接管理</a>}
                  />
                ) : null}
                {grantRows.length > 0 ? (
                  <ul className="home__grant-list">
                    {grantRows.map((grant) => (
                      <li className="home__grant-item" key={grant.connection.id}>
                        <span className="home__grant-name">{grant.connection.name}</span>
                        <span className="home__grant-protocol">
                          {grant.connection.protocolDisplayName}
                        </span>
                        <label className="home__grant-toggle">
                          <input
                            type="checkbox"
                            role="switch"
                            aria-label={`授权 ${grant.connection.name}`}
                            checked={grant.allowed}
                            disabled={savingGrantId !== null}
                            onChange={() => void handleToggleGrant(grant)}
                          />
                          <span>{grant.allowed ? "已授权" : "未授权"}</span>
                        </label>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </>
            )}
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <h3>复制 MCP 配置</h3>
              <StepStatus done={step4Done} />
            </div>

            {tokenSecret === "" ? (
              <>
                <div className="home__notice home__notice--warning" role="note">
                  当前没有可用的 Token 明文（明文仅在创建时显示一次）。请前往{" "}
                  <a href="#/access-control">访问控制</a> 重新创建 Token，或在下方粘贴已有的
                  Token；不会展示或生成伪造 Token。
                </div>
                <div className="form-field">
                  <label className="form-field__label" htmlFor="home-paste-token">
                    粘贴 Token 明文
                  </label>
                  <input
                    id="home-paste-token"
                    className="form-field__control"
                    value={pastedToken}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="umt_..."
                    onChange={(event) => setPastedToken(event.target.value)}
                  />
                </div>
              </>
            ) : (
              <>
                {created === null && pastedToken.trim() !== "" ? (
                  <p className="quickstart__text">
                    正在使用手动粘贴的 Token，请确认其来源与有效性。
                  </p>
                ) : null}
                <pre className="home__code">
                  <code data-testid="mcp-config">
                    {configTokenVisible ? configText : maskedConfigText}
                  </code>
                </pre>
              </>
            )}

            <div className="home__actions">
              <button
                type="button"
                className="btn btn--ghost btn--small"
                aria-pressed={configTokenVisible}
                onClick={() => setConfigTokenVisible((visible) => !visible)}
                disabled={tokenSecret === ""}
              >
                {configTokenVisible ? "隐藏 Token" : "显示 Token"}
              </button>
              <button
                type="button"
                className="btn btn--primary btn--small"
                onClick={() => void handleCopyConfig()}
                disabled={configText === ""}
              >
                复制配置
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--small"
                onClick={handleRegenerate}
              >
                重新生成配置
              </button>
            </div>
          </li>

          <li className="quickstart__step">
            <div className="quickstart__head">
              <h3>复制 AI 接入提示词</h3>
              <StepStatus done={step5Done} />
            </div>
            <p className="quickstart__text">
              提示词只包含服务地址与操作规则，不包含真实 Token，可安全粘贴给 AI 客户端。
            </p>
            <pre className="home__code home__code--prompt">
              <code data-testid="ai-prompt">
                {promptText === "" ? "正在获取 MCP 服务地址…" : promptText}
              </code>
            </pre>
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
