import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AuditDto,
  ConnectionDto,
  GrantDto,
  McpStatusDto,
  OverviewDto,
  TokenDto,
} from "../../api/types";
import HomePage from "./HomePage";

const api = vi.hoisted(() => ({
  getOverview: vi.fn(),
  startMcp: vi.fn(),
  stopMcp: vi.fn(),
  listTokens: vi.fn(),
  createToken: vi.fn(),
  listConnections: vi.fn(),
  listTokenGrants: vi.fn(),
  setTokenGrant: vi.fn(),
}));

vi.mock("../../api/client", () => api);

const ENDPOINT = "http://127.0.0.1:17321/mcp";
const PLAINTEXT = "umt_newpre_plaintext";

function makeMcp(overrides: Partial<McpStatusDto> = {}): McpStatusDto {
  return {
    running: false,
    host: "127.0.0.1",
    port: 17321,
    endpoint: ENDPOINT,
    lastError: null,
    ...overrides,
  };
}

function makeConnection(overrides: Partial<ConnectionDto> = {}): ConnectionDto {
  return {
    id: "c1",
    name: "Production Linux",
    protocolId: "ssh",
    protocolDisplayName: "SSH",
    description: null,
    tags: [],
    notes: null,
    enabled: true,
    capabilities: [],
    hasSecret: true,
    createdAt: 1_760_000_000_000,
    updatedAt: 1_760_000_000_000,
    revision: 1,
    ...overrides,
  };
}

function makeToken(overrides: Partial<TokenDto> = {}): TokenDto {
  return {
    id: "t1",
    name: "Claude Code",
    description: null,
    prefix: "abc123",
    status: "active",
    expiresAt: null,
    createdAt: 1_760_000_000_000,
    lastUsedAt: null,
    ...overrides,
  };
}

function makeAudit(overrides: Partial<AuditDto> = {}): AuditDto {
  return {
    id: 1,
    sessionId: "sess-1",
    tokenId: "t1",
    connectionId: "c1",
    kind: "command_executed",
    outcome: "allowed",
    command: "echo hello",
    workingDir: "/home/demo",
    startedAt: new Date("2026-11-01T09:30:00").getTime(),
    durationMs: 120,
    state: "succeeded",
    exitCode: 0,
    stdoutExcerpt: "hello",
    stderrExcerpt: null,
    error: null,
    redacted: false,
    ...overrides,
  };
}

function makeOverview(overrides: Partial<OverviewDto> = {}): OverviewDto {
  return {
    connectionCount: 2,
    enabledConnectionCount: 1,
    tokenCount: 0,
    activeSessionCount: 3,
    mcp: makeMcp(),
    recentAudits: [makeAudit()],
    ...overrides,
  };
}

function makeGrant(overrides: Partial<GrantDto> = {}): GrantDto {
  return {
    connection: makeConnection(),
    allowed: false,
    grantedAt: null,
    ...overrides,
  };
}

function makeCreatedToken(name: string): { token: TokenDto; plaintext: string } {
  return {
    token: makeToken({ id: "t-new", name, prefix: "newpre" }),
    plaintext: PLAINTEXT,
  };
}

function scopeToStep(title: string): HTMLElement {
  const heading = screen.getByRole("heading", { name: title });
  const step = heading.closest("li");
  if (!step) {
    throw new Error(`未找到步骤：${title}`);
  }
  return step;
}

let writeTextMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  writeTextMock = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(window.navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });

  api.getOverview.mockResolvedValue(makeOverview());
  api.listTokens.mockResolvedValue({ items: [], total: 0 });
  api.listConnections.mockResolvedValue({ items: [makeConnection()], total: 1 });
  api.listTokenGrants.mockResolvedValue([]);
  api.createToken.mockImplementation(async (input: { name: string }) =>
    makeCreatedToken(input.name),
  );
  api.startMcp.mockResolvedValue(makeMcp({ running: true }));
  api.stopMcp.mockResolvedValue(makeMcp({ running: false }));
  api.setTokenGrant.mockImplementation(
    async (input: { connectionId: string; allowed: boolean }) =>
      makeGrant({ allowed: input.allowed, grantedAt: input.allowed ? 1 : null }),
  );
});

describe("首页 - 概览", () => {
  it("渲染统计卡片、MCP 状态与最近审计", async () => {
    api.getOverview.mockResolvedValue(
      makeOverview({ mcp: makeMcp({ running: true }) }),
    );

    render(<HomePage />);

    const connectionLabel = await screen.findByText("连接总数");
    expect(connectionLabel.parentElement).toHaveTextContent("2");
    expect(screen.getByText("已启用连接").parentElement).toHaveTextContent("1");
    expect(screen.getByText("Token 数").parentElement).toHaveTextContent("0");
    expect(screen.getByText("活跃会话").parentElement).toHaveTextContent("3");

    expect((await screen.findAllByText(ENDPOINT)).length).toBeGreaterThan(0);
    expect(screen.getByText("运行中")).toBeInTheDocument();
    expect(screen.getByText("命令执行")).toBeInTheDocument();
    expect(screen.getByText("允许")).toBeInTheDocument();
    expect(screen.getByText("echo hello")).toBeInTheDocument();
  });
});

describe("首页 - MCP 服务", () => {
  it("启动与停止服务后刷新状态", async () => {
    render(<HomePage />);

    const startButton = await screen.findByRole("button", { name: "启动服务" });
    fireEvent.click(startButton);

    await waitFor(() => expect(api.startMcp).toHaveBeenCalledTimes(1));
    const stopButton = screen.getByRole("button", { name: "停止服务" });
    await waitFor(() => expect(stopButton).toBeEnabled());

    fireEvent.click(stopButton);
    await waitFor(() => expect(api.stopMcp).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "启动服务" })).toBeEnabled(),
    );
  });

  it("启动失败时用 toUserMessage 展示端口占用错误", async () => {
    api.startMcp.mockRejectedValue({
      code: "conflict",
      message: "端口 17321 已被占用",
    });

    render(<HomePage />);
    fireEvent.click(await screen.findByRole("button", { name: "启动服务" }));

    const alert = await screen.findByText(/端口 17321 已被占用/);
    expect(alert).toBeInTheDocument();
    expect(alert).toHaveTextContent("数据已被其他操作修改");
  });
});

describe("首页 - 快速开始", () => {
  it("创建 Token 后生成含明文的配置，复制提示词不含 Token", async () => {
    render(<HomePage />);

    fireEvent.change(await screen.findByLabelText("Token 名称"), {
      target: { value: "AI Client" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建 Token" }));

    await waitFor(() =>
      expect(api.createToken).toHaveBeenCalledWith({ name: "AI Client" }),
    );
    expect(await screen.findByText(/明文仅显示一次/)).toBeInTheDocument();

    const step4 = scopeToStep("复制 MCP 配置");
    const maskedConfig = within(step4).getByTestId("mcp-config");
    expect(maskedConfig).not.toHaveTextContent(PLAINTEXT);

    fireEvent.click(within(step4).getByRole("button", { name: "显示 Token" }));
    expect(within(step4).getByTestId("mcp-config")).toHaveTextContent(PLAINTEXT);
    expect(within(step4).getByTestId("mcp-config")).toHaveTextContent(
      `"transport": "streamable-http"`,
    );
    expect(within(step4).getByTestId("mcp-config")).toHaveTextContent(ENDPOINT);

    fireEvent.click(within(step4).getByRole("button", { name: "复制配置" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    const copiedConfig = writeTextMock.mock.calls[0][0] as string;
    expect(copiedConfig).toContain(`Bearer ${PLAINTEXT}`);
    expect(copiedConfig).toContain(ENDPOINT);

    fireEvent.click(screen.getByRole("button", { name: "复制接入提示词" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(2));
    const copiedPrompt = writeTextMock.mock.calls[1][0] as string;
    expect(copiedPrompt).not.toContain(PLAINTEXT);
    expect(copiedPrompt).toContain("1. 只能使用 Universal MCP Terminal");
    expect(copiedPrompt).toContain("10. 发生连接失败或执行异常时");
    expect(await screen.findByText("AI 接入提示词已复制到剪贴板。")).toBeInTheDocument();
  });

  it("没有明文 Token 时不伪造 Token，可粘贴后生成配置", async () => {
    api.getOverview.mockResolvedValue(makeOverview({ tokenCount: 1 }));
    api.listTokens.mockResolvedValue({ items: [makeToken()], total: 1 });

    render(<HomePage />);

    expect(await screen.findByText(/当前没有可用的 Token 明文/)).toBeInTheDocument();
    expect(screen.queryByTestId("mcp-config")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("粘贴 Token 明文"), {
      target: { value: "umt_pasted_token" },
    });

    const step4 = scopeToStep("复制 MCP 配置");
    expect(await within(step4).findByTestId("mcp-config")).toBeInTheDocument();
    fireEvent.click(within(step4).getByRole("button", { name: "显示 Token" }));
    expect(within(step4).getByTestId("mcp-config")).toHaveTextContent(
      "Bearer umt_pasted_token",
    );

    fireEvent.click(screen.getByRole("button", { name: "复制接入提示词" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    expect(writeTextMock.mock.calls[0][0] as string).not.toContain("umt_pasted_token");
  });

  it("切换连接授权时调用 setTokenGrant 并以返回值为准", async () => {
    api.getOverview.mockResolvedValue(makeOverview({ tokenCount: 1 }));
    api.listTokens.mockResolvedValue({ items: [makeToken()], total: 1 });
    api.listTokenGrants.mockResolvedValue([makeGrant()]);

    render(<HomePage />);

    const toggle = await screen.findByRole("switch", {
      name: "授权 Production Linux",
    });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("未授权")).toBeInTheDocument();

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(api.setTokenGrant).toHaveBeenCalledWith({
        tokenId: "t1",
        connectionId: "c1",
        allowed: true,
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "授权 Production Linux" }),
      ).toBeChecked(),
    );
    expect(screen.getByText("已授权")).toBeInTheDocument();
  });

  it("未填写 Token 名称时阻止创建", async () => {
    render(<HomePage />);

    fireEvent.click(await screen.findByRole("button", { name: "创建 Token" }));

    expect(await screen.findByText("Token 名称为必填项")).toBeInTheDocument();
    expect(api.createToken).not.toHaveBeenCalled();
  });
});
