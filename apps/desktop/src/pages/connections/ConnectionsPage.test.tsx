import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../api/client";
import type { ConnectionDetailDto, ConnectionDto, ProtocolInfoDto } from "../../api/types";
import ConnectionsPage from "./ConnectionsPage";

vi.mock("../../api/client", () => ({
  listProtocols: vi.fn(),
  listConnections: vi.fn(),
  getConnection: vi.fn(),
  createConnection: vi.fn(),
  updateConnection: vi.fn(),
  deleteConnection: vi.fn(),
  setConnectionEnabled: vi.fn(),
  duplicateConnection: vi.fn(),
  testConnection: vi.fn(),
  testConnectionDraft: vi.fn(),
}));

const sshProtocol: ProtocolInfoDto = {
  protocolId: "ssh",
  displayName: "SSH",
  version: "1.0.0",
  description: "通过 SSH 连接远程主机",
  capabilities: ["exec", "shell"],
  configSchema: {
    protocolId: "ssh",
    fields: [
      { key: "host", label: "主机", kind: { kind: "text" }, required: true, secret: false },
      {
        key: "port",
        label: "端口",
        kind: { kind: "number" },
        required: false,
        secret: false,
        defaultValue: 22,
      },
      {
        key: "password",
        label: "密码",
        kind: { kind: "password" },
        required: true,
        secret: true,
      },
    ],
  },
};

const telnetProtocol: ProtocolInfoDto = {
  protocolId: "telnet",
  displayName: "Telnet",
  version: "1.0.0",
  description: "通过 Telnet 连接网络设备",
  capabilities: ["exec"],
  configSchema: {
    protocolId: "telnet",
    fields: [
      { key: "host", label: "主机", kind: { kind: "text" }, required: true, secret: false },
      {
        key: "port",
        label: "端口",
        kind: { kind: "number" },
        required: false,
        secret: false,
        defaultValue: 23,
      },
    ],
  },
};

function makeConnection(
  partial: Partial<ConnectionDto> & { id: string; name: string },
): ConnectionDto {
  return {
    protocolId: "ssh",
    protocolDisplayName: "SSH",
    description: null,
    tags: [],
    notes: null,
    enabled: true,
    capabilities: ["exec"],
    hasSecret: true,
    createdAt: Date.UTC(2026, 0, 1, 8, 0, 0),
    updatedAt: Date.UTC(2026, 0, 2, 9, 30, 0),
    revision: 1,
    ...partial,
  };
}

function makeDetail(connection: ConnectionDto): ConnectionDetailDto {
  return {
    connection,
    config: { host: "10.0.0.5" },
    secretFieldsSet: ["password"],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.listProtocols).mockResolvedValue([sshProtocol, telnetProtocol]);
  vi.mocked(api.listConnections).mockResolvedValue({ items: [], total: 0 });
  vi.mocked(api.getConnection).mockImplementation(async (id) =>
    makeDetail(makeConnection({ id, name: "测试连接" })),
  );
  vi.mocked(api.deleteConnection).mockResolvedValue(true);
});

describe("ConnectionsPage", () => {
  it("渲染连接列表、协议展示名、能力标签与分页信息", async () => {
    vi.mocked(api.listConnections).mockResolvedValue({
      items: [
        makeConnection({
          id: "c1",
          name: "生产服务器",
          description: "主要生产环境",
          tags: ["prod"],
          capabilities: ["exec", "shell"],
        }),
        makeConnection({
          id: "c2",
          name: "实验环境",
          protocolId: "telnet",
          protocolDisplayName: "Telnet",
          capabilities: ["exec"],
        }),
      ],
      total: 2,
    });

    render(<ConnectionsPage />);

    expect(await screen.findByText("生产服务器")).toBeInTheDocument();
    expect(screen.getByText("实验环境")).toBeInTheDocument();
    const table = screen.getByRole("table");
    expect(within(table).getByText("SSH")).toBeInTheDocument();
    expect(within(table).getByText("Telnet")).toBeInTheDocument();
    expect(screen.getByText("主要生产环境")).toBeInTheDocument();
    expect(screen.getByText("shell")).toBeInTheDocument();
    expect(screen.getByText(/第 1-2 条，共 2 条/)).toBeInTheDocument();

    expect(api.listConnections).toHaveBeenCalledWith({
      offset: 0,
      limit: 20,
      protocolId: null,
      enabled: null,
      search: null,
    });
  });

  it("搜索防抖后请求，并将协议与启用状态筛选传给后端", async () => {
    render(<ConnectionsPage />);
    expect(await screen.findByText("暂无连接")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("搜索连接"), {
      target: { value: "生产" },
    });
    await waitFor(
      () => {
        expect(api.listConnections).toHaveBeenLastCalledWith(
          expect.objectContaining({ search: "生产", offset: 0, limit: 20 }),
        );
      },
      { timeout: 3000 },
    );

    fireEvent.change(screen.getByLabelText("按协议筛选"), {
      target: { value: "ssh" },
    });
    await waitFor(() => {
      expect(api.listConnections).toHaveBeenLastCalledWith(
        expect.objectContaining({ protocolId: "ssh", search: "生产" }),
      );
    });

    fireEvent.change(screen.getByLabelText("按启用状态筛选"), {
      target: { value: "disabled" },
    });
    await waitFor(() => {
      expect(api.listConnections).toHaveBeenLastCalledWith(
        expect.objectContaining({ enabled: false, protocolId: "ssh", search: "生产" }),
      );
    });
  });

  it("没有数据时展示空态与创建入口", async () => {
    render(<ConnectionsPage />);

    expect(await screen.findByText("暂无连接")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接" })).toBeInTheDocument();
  });

  it("切换启用开关调用 setConnectionEnabled 并刷新列表", async () => {
    const target = makeConnection({ id: "c1", name: "生产服务器", enabled: true });
    vi.mocked(api.listConnections).mockResolvedValue({ items: [target], total: 1 });
    vi.mocked(api.setConnectionEnabled).mockResolvedValue({ ...target, enabled: false });

    render(<ConnectionsPage />);

    const toggle = await screen.findByRole("switch", { name: "启用 生产服务器" });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(api.setConnectionEnabled).toHaveBeenCalledWith("c1", false);
    });
    await waitFor(() => {
      expect(api.listConnections).toHaveBeenCalledTimes(2);
    });
  });

  it("新建连接按协议 schema 提交配置与 secrets", async () => {
    vi.mocked(api.createConnection).mockResolvedValue(
      makeConnection({ id: "c9", name: "测试机" }),
    );
    render(<ConnectionsPage />);
    expect(await screen.findByText("暂无连接")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新建连接" }));
    const dialog = screen.getByRole("dialog", { name: "新建连接" });

    fireEvent.change(within(dialog).getByLabelText("名称"), {
      target: { value: "测试机" },
    });
    fireEvent.change(within(dialog).getByLabelText("协议"), {
      target: { value: "ssh" },
    });
    fireEvent.change(within(dialog).getByLabelText("标签"), {
      target: { value: "test, 临时" },
    });
    fireEvent.change(await within(dialog).findByLabelText("主机"), {
      target: { value: "10.0.0.8" },
    });
    fireEvent.change(within(dialog).getByLabelText("密码"), {
      target: { value: "s3cret" },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "创建连接" }));

    await waitFor(() => expect(api.createConnection).toHaveBeenCalledTimes(1));
    const payload = vi.mocked(api.createConnection).mock.calls[0][0];
    expect(payload).toMatchObject({
      name: "测试机",
      protocolId: "ssh",
      tags: ["test", "临时"],
      enabled: true,
      secrets: [{ key: "password", value: "s3cret" }],
    });
    expect(payload.config).toMatchObject({ host: "10.0.0.8", port: 22 });
    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "新建连接" }),
      ).not.toBeInTheDocument();
    });
  });

  it("编辑连接回填非敏感配置，敏感字段留空时 secrets 传 null", async () => {
    const existing = makeConnection({
      id: "c1",
      name: "生产服务器",
      description: "主环境",
      tags: ["prod"],
      notes: "值班电话",
      revision: 3,
    });
    vi.mocked(api.listConnections).mockResolvedValue({ items: [existing], total: 1 });
    vi.mocked(api.getConnection).mockResolvedValue({
      connection: existing,
      config: { host: "10.0.0.5", port: 2222 },
      secretFieldsSet: ["password"],
    });
    vi.mocked(api.updateConnection).mockResolvedValue({ ...existing, revision: 4 });

    render(<ConnectionsPage />);
    await screen.findByText("生产服务器");
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));

    const dialog = await screen.findByRole("dialog", { name: "编辑连接" });
    await waitFor(() => {
      expect(within(dialog).getByLabelText("名称")).toHaveValue("生产服务器");
    });
    expect(within(dialog).getByLabelText("协议")).toBeDisabled();
    expect(within(dialog).getByLabelText("主机")).toHaveValue("10.0.0.5");
    expect(within(dialog).getByLabelText("密码")).toHaveValue("");
    expect(within(dialog).getByText("敏感字段已加密保存，留空表示不修改。")).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "保存修改" }));

    await waitFor(() => expect(api.updateConnection).toHaveBeenCalledTimes(1));
    expect(vi.mocked(api.updateConnection).mock.calls[0][0]).toMatchObject({
      id: "c1",
      secrets: null,
      expectedRevision: 3,
      config: { host: "10.0.0.5", port: 2222 },
    });
  });

  it("保存遇到 conflict 时提示刷新后重试", async () => {
    vi.mocked(api.createConnection).mockRejectedValue({
      code: "conflict",
      message: "revision mismatch",
    });
    render(<ConnectionsPage />);
    expect(await screen.findByText("暂无连接")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新建连接" }));
    const dialog = screen.getByRole("dialog", { name: "新建连接" });
    fireEvent.change(within(dialog).getByLabelText("名称"), {
      target: { value: "测试机" },
    });
    fireEvent.change(within(dialog).getByLabelText("协议"), {
      target: { value: "ssh" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "创建连接" }));

    expect(
      await screen.findByText("连接已被其他操作修改，请刷新后重试。"),
    ).toBeInTheDocument();
  });

  it("弹窗内测试连接失败时展示步骤、耗时与建议", async () => {
    vi.mocked(api.testConnectionDraft).mockResolvedValue({
      ok: false,
      message: "SSH 认证失败",
      latencyMs: 321,
      steps: [
        { name: "建立 TCP 连接", status: "passed", durationMs: 20 },
        { name: "SSH 认证", status: "failed", message: "密码错误", durationMs: 300 },
      ],
      suggestion: "请检查用户名和密码是否正确",
    });

    render(<ConnectionsPage />);
    expect(await screen.findByText("暂无连接")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新建连接" }));
    const dialog = screen.getByRole("dialog", { name: "新建连接" });
    fireEvent.change(within(dialog).getByLabelText("协议"), {
      target: { value: "ssh" },
    });
    fireEvent.change(await within(dialog).findByLabelText("主机"), {
      target: { value: "10.0.0.5" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "测试连接" }));

    expect(await within(dialog).findByText("连接测试失败")).toBeInTheDocument();
    expect(within(dialog).getByText("SSH 认证失败")).toBeInTheDocument();
    expect(within(dialog).getByText("耗时 321 ms")).toBeInTheDocument();
    expect(within(dialog).getByText("密码错误")).toBeInTheDocument();
    expect(
      within(dialog).getByText("建议：请检查用户名和密码是否正确"),
    ).toBeInTheDocument();
    expect(api.testConnectionDraft).toHaveBeenCalledWith(
      expect.objectContaining({
        protocolId: "ssh",
        config: expect.objectContaining({ host: "10.0.0.5" }),
        secrets: [],
      }),
    );
  });

  it("弹窗内测试连接请求失败时展示错误提示", async () => {
    vi.mocked(api.testConnectionDraft).mockRejectedValue({
      code: "validation",
      message: "host 不能为空",
    });

    render(<ConnectionsPage />);
    expect(await screen.findByText("暂无连接")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新建连接" }));
    const dialog = screen.getByRole("dialog", { name: "新建连接" });
    fireEvent.change(within(dialog).getByLabelText("协议"), {
      target: { value: "ssh" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "测试连接" }));

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      /提交的内容不合法/,
    );
  });

  it("删除连接需要二次确认", async () => {
    const existing = makeConnection({ id: "c1", name: "生产服务器" });
    vi.mocked(api.listConnections).mockResolvedValue({ items: [existing], total: 1 });
    vi.mocked(api.getConnection).mockResolvedValue({
      connection: existing,
      config: { host: "10.0.0.5", port: 22 },
      secretFieldsSet: ["password"],
    });

    render(<ConnectionsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "生产服务器" }));

    expect(
      await screen.findByRole("heading", { name: "生产服务器" }),
    ).toBeInTheDocument();
    expect(screen.getByText("已保存：密码")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    const confirm = await screen.findByRole("dialog", { name: "删除连接" });
    expect(api.deleteConnection).not.toHaveBeenCalled();

    fireEvent.click(within(confirm).getByRole("button", { name: "删除" }));

    await waitFor(() => {
      expect(api.deleteConnection).toHaveBeenCalledWith("c1");
    });
    expect(await screen.findByText("已删除连接「生产服务器」")).toBeInTheDocument();
  });
});
