import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AuditDto,
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
  getTokenMcpConfig: vi.fn(),
}));

vi.mock("../../api/client", () => api);

const ENDPOINT = "http://127.0.0.1:47321/mcp";
const PLAINTEXT = "umt_newpre_plaintext";

function makeConfig(secret: string): string {
  return [
    "{",
    '  "mcpServers": {',
    '    "universal-mcp-terminal": {',
    '      "transport": "streamable-http",',
    `      "url": "${ENDPOINT}",`,
    '      "headers": {',
    `        "Authorization": "Bearer ${secret}"`,
    "      }",
    "    }",
    "  }",
    "}",
  ].join("\n");
}

function makeMcp(overrides: Partial<McpStatusDto> = {}): McpStatusDto {
  return {
    running: false,
    host: "127.0.0.1",
    port: 47321,
    endpoint: ENDPOINT,
    lastError: null,
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
  api.createToken.mockImplementation(async (input: { name: string }) =>
    makeCreatedToken(input.name),
  );
  api.getTokenMcpConfig.mockResolvedValue(makeConfig(PLAINTEXT));
  api.startMcp.mockResolvedValue(makeMcp({ running: true }));
  api.stopMcp.mockResolvedValue(makeMcp({ running: false }));
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
      message: "端口 47321 已被占用",
    });

    render(<HomePage />);
    fireEvent.click(await screen.findByRole("button", { name: "启动服务" }));

    const alert = await screen.findByText(/端口 47321 已被占用/);
    expect(alert).toBeInTheDocument();
    expect(alert).toHaveTextContent("数据已被其他操作修改");
  });
});

describe("首页 - 快速开始", () => {
  it("创建 Token 后自动生成配置，提示词包含该配置", async () => {
    const config = makeConfig(PLAINTEXT);
    api.getTokenMcpConfig.mockResolvedValue(config);

    render(<HomePage />);

    fireEvent.change(await screen.findByLabelText("Token 名称"), {
      target: { value: "AI Client" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建 Token" }));

    await waitFor(() =>
      expect(api.createToken).toHaveBeenCalledWith({ name: "AI Client" }),
    );
    expect(await screen.findByText(/Token 已创建/)).toBeInTheDocument();
    await waitFor(() =>
      expect(api.getTokenMcpConfig).toHaveBeenCalledWith("t-new"),
    );

    const step3 = scopeToStep("选择访问 Token");
    const shownConfig = within(step3).getByTestId("mcp-config");
    await waitFor(() => expect(shownConfig).toHaveTextContent(PLAINTEXT));
    expect(shownConfig).toHaveTextContent(`"transport": "streamable-http"`);
    expect(shownConfig).toHaveTextContent(ENDPOINT);

    const step4 = scopeToStep("复制提示词");
    const prompt = within(step4).getByTestId("ai-prompt");
    expect(prompt).toHaveTextContent(PLAINTEXT);
    expect(prompt).toHaveTextContent("tools/list");

    fireEvent.click(within(step3).getByRole("button", { name: "复制配置" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    expect(writeTextMock.mock.calls[0][0]).toBe(config);

    fireEvent.click(within(step4).getByRole("button", { name: "复制接入提示词" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(2));
    const copiedPrompt = writeTextMock.mock.calls[1][0] as string;
    expect(copiedPrompt).toContain(`Bearer ${PLAINTEXT}`);
    expect(copiedPrompt).toContain('"universal-mcp-terminal"');
    expect(copiedPrompt).toContain("tools/list");
    expect(await screen.findByText("AI 接入提示词已复制到剪贴板。")).toBeInTheDocument();
  });

  it("已有 Token 时无需新建，直接生成配置和提示词", async () => {
    const config = makeConfig("umt_existing_token");
    api.getOverview.mockResolvedValue(makeOverview({ tokenCount: 1 }));
    api.listTokens.mockResolvedValue({ items: [makeToken()], total: 1 });
    api.getTokenMcpConfig.mockResolvedValue(config);

    render(<HomePage />);

    const select = await screen.findByLabelText("选择访问 Token");
    expect(select).toHaveValue("t1");
    await waitFor(() =>
      expect(api.getTokenMcpConfig).toHaveBeenCalledWith("t1"),
    );

    const step3 = scopeToStep("选择访问 Token");
    await waitFor(() =>
      expect(within(step3).getByTestId("mcp-config")).toHaveTextContent(
        "umt_existing_token",
      ),
    );
    expect(screen.queryByText(/无法获取密钥/)).not.toBeInTheDocument();

    const step4 = scopeToStep("复制提示词");
    const prompt = within(step4).getByTestId("ai-prompt");
    expect(prompt).toHaveTextContent("umt_existing_token");
    fireEvent.click(within(step4).getByRole("button", { name: "复制接入提示词" }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledTimes(1));
    expect(writeTextMock.mock.calls[0][0] as string).toContain(config);
  });

  it("切换 Token 时按所选 Token 重新生成配置", async () => {
    api.getOverview.mockResolvedValue(makeOverview({ tokenCount: 2 }));
    api.listTokens.mockResolvedValue({
      items: [
        makeToken(),
        makeToken({ id: "t2", name: "Cursor", prefix: "cursor" }),
      ],
      total: 2,
    });
    api.getTokenMcpConfig.mockImplementation(async (tokenId: string) =>
      makeConfig(tokenId === "t2" ? "umt_cursor_token" : "umt_claude_token"),
    );

    render(<HomePage />);

    const step3 = scopeToStep("选择访问 Token");
    const select = await within(step3).findByLabelText("选择访问 Token");
    await waitFor(() =>
      expect(within(step3).getByTestId("mcp-config")).toHaveTextContent(
        "umt_claude_token",
      ),
    );

    fireEvent.change(select, { target: { value: "t2" } });
    await waitFor(() =>
      expect(within(step3).getByTestId("mcp-config")).toHaveTextContent(
        "umt_cursor_token",
      ),
    );
    expect(api.getTokenMcpConfig).toHaveBeenLastCalledWith("t2");
  });

  it("旧版本 Token 无法生成配置时给出提示且禁止复制", async () => {
    api.getOverview.mockResolvedValue(makeOverview({ tokenCount: 1 }));
    api.listTokens.mockResolvedValue({ items: [makeToken()], total: 1 });
    api.getTokenMcpConfig.mockRejectedValue({
      code: "validation",
      message: "该 Token 创建于旧版本，无法获取密钥，请在访问控制中重新创建",
    });

    render(<HomePage />);

    expect(await screen.findByText(/创建于旧版本/)).toBeInTheDocument();

    const step3 = scopeToStep("选择访问 Token");
    expect(
      within(step3).getByRole("button", { name: "复制配置" }),
    ).toBeDisabled();
    const step4 = scopeToStep("复制提示词");
    expect(
      within(step4).getByRole("button", { name: "复制接入提示词" }),
    ).toBeDisabled();
  });

  it("未填写 Token 名称时阻止创建", async () => {
    render(<HomePage />);

    fireEvent.click(await screen.findByRole("button", { name: "创建 Token" }));

    expect(await screen.findByText("Token 名称为必填项")).toBeInTheDocument();
    expect(api.createToken).not.toHaveBeenCalled();
  });
});
