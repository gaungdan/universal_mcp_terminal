import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as client from "../../api/client";
import type { ConnectionDto, GrantDto, TokenDto } from "../../api/types";
import AccessControlPage from "./AccessControlPage";

vi.mock("../../api/client", () => ({
  listTokens: vi.fn(),
  createToken: vi.fn(),
  setTokenStatus: vi.fn(),
  deleteToken: vi.fn(),
  listTokenGrants: vi.fn(),
  setTokenGrant: vi.fn(),
}));

const sshConnection: ConnectionDto = {
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
};

function makeToken(overrides: Partial<TokenDto> = {}): TokenDto {
  return {
    id: "t1",
    name: "Claude Code",
    description: null,
    prefix: "abc123",
    status: "active",
    permission: "read_only",
    allowInteractive: false,
    expiresAt: null,
    createdAt: 1_760_000_000_000,
    lastUsedAt: null,
    ...overrides,
  };
}

function makeGrant(overrides: Partial<GrantDto> = {}): GrantDto {
  return {
    connection: sshConnection,
    allowed: false,
    grantedAt: null,
    ...overrides,
  };
}

function deferred<T>() {
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((_resolve, rejectPromise) => {
    reject = rejectPromise;
  });
  return { promise, reject };
}

function renderPage() {
  return render(<AccessControlPage />);
}

let writeTextMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  writeTextMock = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(window.navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });

  vi.mocked(client.listTokens).mockResolvedValue({ items: [makeToken()], total: 1 });
  vi.mocked(client.listTokenGrants).mockResolvedValue([]);
  vi.mocked(client.createToken).mockResolvedValue({
    token: makeToken({ id: "t-new", name: "New Token", prefix: "newpre" }),
    plaintext: "umt_newpre_plaintext",
  });
  vi.mocked(client.setTokenStatus).mockImplementation(async (input) =>
    makeToken({ id: input.id, status: input.status }),
  );
  vi.mocked(client.deleteToken).mockResolvedValue(true);
  vi.mocked(client.setTokenGrant).mockImplementation(async (input) => ({
    connection: sshConnection,
    allowed: input.allowed,
    grantedAt: input.allowed ? 1_760_000_100_000 : null,
  }));
});

describe("访问控制页 - Token 列表", () => {
  it("渲染名称、脱敏 Token、状态、有效期与最后使用时间", async () => {
    renderPage();

    expect(await screen.findByRole("button", { name: "Claude Code" })).toBeInTheDocument();
    expect(screen.getByText("umt_abc123_••••••")).toBeInTheDocument();
    expect(screen.getByText("生效中")).toBeInTheDocument();
    expect(screen.getByText("永久有效")).toBeInTheDocument();
    expect(screen.getByText("从未使用")).toBeInTheDocument();
    expect(screen.getByText("共 1 条")).toBeInTheDocument();
    expect(screen.getByRole("table")).toBeInTheDocument();
  });

  it("使用 PageDto.total 计算分页并请求下一页", async () => {
    vi.mocked(client.listTokens).mockResolvedValue({ items: [makeToken()], total: 25 });

    renderPage();

    await screen.findByRole("button", { name: "Claude Code" });
    expect(screen.getByText("共 25 条")).toBeInTheDocument();
    expect(screen.getByText("第 1 / 3 页")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上一页" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下一页" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "下一页" }));

    await waitFor(() =>
      expect(client.listTokens).toHaveBeenCalledWith({ offset: 10, limit: 10 }),
    );
  });

  it("没有 Token 时展示空态并引导创建", async () => {
    vi.mocked(client.listTokens).mockResolvedValue({ items: [], total: 0 });

    renderPage();

    expect(await screen.findByText("还没有访问 Token")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建第一个 Token" }));

    expect(await screen.findByRole("dialog", { name: "新建 Token" })).toBeInTheDocument();
  });

  it("access_denied 错误转换为友好提示", async () => {
    vi.mocked(client.listTokens).mockRejectedValue({
      code: "access_denied",
      message: "当前账号无权限",
    });

    renderPage();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("没有权限执行此操作");
    expect(alert).toHaveTextContent("当前账号无权限");
  });
});

describe("访问控制页 - 创建 Token", () => {
  it("创建后一次性展示明文，复制并关闭后不再展示", async () => {
    renderPage();
    await screen.findByRole("button", { name: "Claude Code" });

    fireEvent.click(screen.getByRole("button", { name: "新建 Token" }));
    const dialog = await screen.findByRole("dialog", { name: "新建 Token" });

    fireEvent.change(within(dialog).getByLabelText("名称"), {
      target: { value: "Ops Bot" },
    });
    fireEvent.change(within(dialog).getByLabelText("描述"), {
      target: { value: "CI 使用" },
    });
    fireEvent.change(within(dialog).getByLabelText("有效期（可选）"), {
      target: { value: "2026-12-31T10:30" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "创建 Token" }));

    await waitFor(() => expect(client.createToken).toHaveBeenCalledTimes(1));
    const input = vi.mocked(client.createToken).mock.calls[0][0];
    expect(input.name).toBe("Ops Bot");
    expect(input.description).toBe("CI 使用");
    expect(input.expiresAt).toBe(new Date("2026-12-31T10:30").getTime());

    const reveal = await screen.findByRole("dialog", { name: "Token 已创建" });
    expect(
      within(reveal).getByText("此 Token 仅显示一次，请立即保存"),
    ).toBeInTheDocument();
    expect(within(reveal).getByTestId("token-plaintext")).toHaveTextContent(
      "umt_newpre_plaintext",
    );

    fireEvent.click(within(reveal).getByRole("button", { name: "复制 Token" }));
    await waitFor(() =>
      expect(writeTextMock).toHaveBeenCalledWith("umt_newpre_plaintext"),
    );
    expect(await within(reveal).findByText("已复制到剪贴板")).toBeInTheDocument();

    fireEvent.click(within(reveal).getByRole("button", { name: "我已保存，关闭" }));

    await waitFor(() =>
      expect(screen.queryByTestId("token-plaintext")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(client.listTokens).toHaveBeenCalledTimes(2));
  });

  it("剪贴板写入失败时提示手动复制", async () => {
    writeTextMock.mockRejectedValue(new Error("clipboard denied"));
    renderPage();
    await screen.findByRole("button", { name: "Claude Code" });

    fireEvent.click(screen.getByRole("button", { name: "新建 Token" }));
    const dialog = await screen.findByRole("dialog", { name: "新建 Token" });
    fireEvent.change(within(dialog).getByLabelText("名称"), {
      target: { value: "Ops Bot" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "创建 Token" }));

    const reveal = await screen.findByRole("dialog", { name: "Token 已创建" });
    fireEvent.click(within(reveal).getByRole("button", { name: "复制 Token" }));

    expect(
      await within(reveal).findByText("复制失败，请手动选中 Token 后复制"),
    ).toBeInTheDocument();
  });
});

describe("访问控制页 - 状态操作", () => {
  it("撤销前二次确认并提示会终止活动会话", async () => {
    const revoked = makeToken({ status: "revoked" });
    vi.mocked(client.listTokens)
      .mockResolvedValueOnce({ items: [makeToken()], total: 1 })
      .mockResolvedValue({ items: [revoked], total: 1 });
    vi.mocked(client.setTokenStatus).mockResolvedValue(revoked);

    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "撤销 Claude Code" }));
    const dialog = screen.getByRole("dialog", { name: "撤销 Token" });
    expect(
      within(dialog).getByText(/撤销后不可恢复，并会立即终止其活动会话/),
    ).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "确认撤销" }));

    await waitFor(() =>
      expect(client.setTokenStatus).toHaveBeenCalledWith({
        id: "t1",
        status: "revoked",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "撤销 Token" })).not.toBeInTheDocument(),
    );
    expect(await screen.findByText("已撤销")).toBeInTheDocument();
  });

  it("删除仅对非 active Token 可用，并需确认", async () => {
    vi.mocked(client.listTokens).mockResolvedValue({
      items: [
        makeToken(),
        makeToken({ id: "t2", name: "CI Runner", prefix: "def456", status: "disabled" }),
      ],
      total: 2,
    });

    renderPage();
    await screen.findByRole("button", { name: "Claude Code" });

    const activeRow = screen
      .getByRole("button", { name: "Claude Code" })
      .closest("tr");
    expect(activeRow).not.toBeNull();
    expect(
      within(activeRow as HTMLElement).getByRole("button", {
        name: "删除 Claude Code",
      }),
    ).toBeDisabled();

    const disabledRow = screen
      .getByRole("button", { name: "CI Runner" })
      .closest("tr");
    expect(disabledRow).not.toBeNull();
    const deleteButton = within(disabledRow as HTMLElement).getByRole("button", {
      name: "删除 CI Runner",
    });
    expect(deleteButton).toBeEnabled();

    fireEvent.click(deleteButton);
    const dialog = screen.getByRole("dialog", { name: "删除 Token" });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认删除" }));

    await waitFor(() => expect(client.deleteToken).toHaveBeenCalledWith("t2"));
  });
});

describe("访问控制页 - 授权矩阵", () => {
  it("切换授权后以后端返回值为准", async () => {
    vi.mocked(client.listTokenGrants).mockResolvedValue([makeGrant()]);
    vi.mocked(client.setTokenGrant).mockResolvedValue(
      makeGrant({ allowed: true, grantedAt: 1_760_000_100_000 }),
    );

    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Claude Code" }));

    const toggle = await screen.findByRole("switch", {
      name: "授权 Production Linux",
    });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("关闭授权会立即阻止该 Token 的新操作。")).toBeInTheDocument();

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(client.setTokenGrant).toHaveBeenCalledWith({
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

  it("切换失败时回滚开关并提示错误", async () => {
    vi.mocked(client.listTokenGrants).mockResolvedValue([makeGrant()]);
    const pending = deferred<GrantDto>();
    vi.mocked(client.setTokenGrant).mockReturnValue(pending.promise);

    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Claude Code" }));

    const toggle = await screen.findByRole("switch", {
      name: "授权 Production Linux",
    });
    fireEvent.click(toggle);

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "授权 Production Linux" }),
      ).toBeChecked(),
    );

    await act(async () => {
      pending.reject({ code: "access_denied", message: "denied by policy" });
    });

    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "授权 Production Linux" }),
      ).not.toBeChecked(),
    );
    expect(screen.getByText(/没有权限执行此操作/)).toBeInTheDocument();
    expect(screen.getByText(/denied by policy/)).toBeInTheDocument();
  });
});
