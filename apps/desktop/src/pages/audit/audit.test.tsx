import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditDto, SessionDto } from "../../api/types";
import AuditPage from "./AuditPage";

const api = vi.hoisted(() => ({
  queryAudit: vi.fn(),
  exportAudit: vi.fn(),
  listSessionHistory: vi.fn(),
  listActiveSessions: vi.fn(),
}));

vi.mock("../../api/client", () => api);

function makeAudit(overrides: Partial<AuditDto> = {}): AuditDto {
  return {
    id: 101,
    sessionId: "sess-1",
    tokenId: "tok-1",
    connectionId: "conn-1",
    kind: "command_executed",
    outcome: "allowed",
    command: "echo hello",
    workingDir: "/home/demo",
    startedAt: new Date("2026-11-01T09:30:00").getTime(),
    durationMs: 1250,
    state: "succeeded",
    exitCode: 0,
    stdoutExcerpt: "hello",
    stderrExcerpt: null,
    error: null,
    redacted: false,
    ...overrides,
  };
}

function makeSession(overrides: Partial<SessionDto> = {}): SessionDto {
  return {
    id: "sess-history-1",
    tokenId: "tok-1",
    connectionId: "conn-1",
    protocolId: "ssh",
    clientInfo: "test-client",
    state: "closed",
    startedAt: new Date("2026-11-01T08:00:00").getTime(),
    endedAt: new Date("2026-11-01T08:30:00").getTime(),
    closeReason: "客户端主动关闭",
    rows: 24,
    cols: 80,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.queryAudit.mockResolvedValue({ items: [], total: 0 });
  api.exportAudit.mockResolvedValue({ path: "C:\\logs\\audit.csv", rows: 3 });
  api.listSessionHistory.mockResolvedValue({ items: [], total: 0 });
  api.listActiveSessions.mockResolvedValue([]);
});

describe("AuditPage 筛选", () => {
  it("把筛选栏内容转换为 UnixMillis 与查询参数", async () => {
    render(<AuditPage />);

    fireEvent.change(screen.getByLabelText("开始时间"), {
      target: { value: "2026-11-01T09:00" },
    });
    fireEvent.change(screen.getByLabelText("结束时间"), {
      target: { value: "2026-11-02T18:30" },
    });
    fireEvent.change(screen.getByLabelText("Token ID"), {
      target: { value: "tok-9" },
    });
    fireEvent.change(screen.getByLabelText("Connection ID"), {
      target: { value: "conn-9" },
    });
    fireEvent.change(screen.getByLabelText("Session ID"), {
      target: { value: "sess-9" },
    });
    fireEvent.change(screen.getByLabelText("操作类型"), {
      target: { value: "permission_denied" },
    });
    fireEvent.change(screen.getByLabelText("执行结果"), {
      target: { value: "denied" },
    });
    fireEvent.click(screen.getByRole("button", { name: "查询" }));

    await waitFor(() => {
      expect(api.queryAudit).toHaveBeenLastCalledWith({
        offset: 0,
        limit: 50,
        tokenId: "tok-9",
        connectionId: "conn-9",
        sessionId: "sess-9",
        kind: "permission_denied",
        outcome: "denied",
        from: new Date("2026-11-01T09:00").getTime(),
        to: new Date("2026-11-02T18:30").getTime(),
      });
    });
  });

  it("重置清空全部筛选条件并重新查询", async () => {
    render(<AuditPage />);

    fireEvent.change(screen.getByLabelText("Token ID"), {
      target: { value: "tok-9" },
    });
    fireEvent.click(screen.getByRole("button", { name: "查询" }));
    await waitFor(() => {
      expect(api.queryAudit).toHaveBeenLastCalledWith(
        expect.objectContaining({ tokenId: "tok-9" }),
      );
    });

    fireEvent.click(screen.getByRole("button", { name: "重置" }));

    expect(screen.getByLabelText("Token ID")).toHaveValue("");
    await waitFor(() => {
      expect(api.queryAudit).toHaveBeenLastCalledWith(
        expect.objectContaining({
          tokenId: null,
          connectionId: null,
          sessionId: null,
          kind: null,
          outcome: null,
          from: null,
          to: null,
        }),
      );
    });
  });

  it("执行状态下拉只在当前页记录内过滤", async () => {
    api.queryAudit.mockResolvedValue({
      items: [
        makeAudit({ id: 1, command: "echo ok", state: "succeeded" }),
        makeAudit({ id: 2, command: "echo bad", state: "failed", exitCode: 1 }),
      ],
      total: 2,
    });
    render(<AuditPage />);

    await screen.findByText("echo ok");
    fireEvent.change(screen.getByLabelText("执行状态"), {
      target: { value: "failed" },
    });
    fireEvent.click(screen.getByRole("button", { name: "查询" }));

    await waitFor(() => {
      const table = screen.getByRole("table");
      expect(within(table).queryByText("echo ok")).not.toBeInTheDocument();
      expect(within(table).getByText("echo bad")).toBeInTheDocument();
    });
  });
});

describe("AuditPage 列表与详情", () => {
  it("渲染审计列表的关键字段", async () => {
    api.queryAudit.mockResolvedValue({ items: [makeAudit()], total: 1 });
    render(<AuditPage />);

    const table = await screen.findByRole("table");
    expect(within(table).getByText("2026-11-01 09:30:00")).toBeInTheDocument();
    expect(within(table).getByText("命令执行")).toBeInTheDocument();
    expect(within(table).getByText("允许")).toBeInTheDocument();
    expect(within(table).getByText("成功")).toBeInTheDocument();
    expect(within(table).getByText("tok-1")).toBeInTheDocument();
    expect(within(table).getByText("conn-1")).toBeInTheDocument();
    expect(within(table).getByText("sess-1")).toBeInTheDocument();
    expect(within(table).getByText("echo hello")).toBeInTheDocument();
    expect(screen.getByText("共 1 条记录，每页 50 条。")).toBeInTheDocument();
  });

  it("点击行后展示完整命令、输出、错误与脱敏标记", async () => {
    api.queryAudit.mockResolvedValue({
      items: [
        makeAudit({
          command: "cat /var/log/app.log",
          stdoutExcerpt: "127.0.0.1 localhost",
          stderrExcerpt: "permission denied line",
          error: "退出码非零",
          workingDir: "/srv/app",
          durationMs: 2500,
          exitCode: 2,
          redacted: true,
        }),
      ],
      total: 1,
    });
    render(<AuditPage />);

    fireEvent.click(await screen.findByText("cat /var/log/app.log"));

    const detail = await screen.findByRole("complementary", { name: "审计详情" });
    expect(within(detail).getByText("已脱敏")).toBeInTheDocument();
    expect(within(detail).getByText("cat /var/log/app.log")).toBeInTheDocument();
    expect(within(detail).getByText("/srv/app")).toBeInTheDocument();
    expect(within(detail).getByText("2.50 s")).toBeInTheDocument();
    expect(within(detail).getByText("退出码非零")).toBeInTheDocument();
    expect(within(detail).getByText("127.0.0.1 localhost")).toBeInTheDocument();
    expect(within(detail).getByText("permission denied line")).toBeInTheDocument();
    expect(within(detail).getByRole("button", { name: "复制命令" })).toBeEnabled();
  });

  it("无记录时提示调整筛选条件", async () => {
    render(<AuditPage />);

    expect(await screen.findByText("没有匹配的审计记录")).toBeInTheDocument();
    expect(screen.getByText("调整筛选条件后重新查询。")).toBeInTheDocument();
  });
});

describe("AuditPage 会话历史", () => {
  it("切换 Tab 后加载会话历史，并显示活跃会话数量徽标", async () => {
    api.listActiveSessions.mockResolvedValue([
      makeSession({ id: "sess-active-1", state: "running", endedAt: null, closeReason: null }),
      makeSession({ id: "sess-active-2", state: "connected", endedAt: null, closeReason: null }),
    ]);
    api.listSessionHistory.mockResolvedValue({
      items: [makeSession({ id: "sess-history-9", closeReason: "服务端超时" })],
      total: 1,
    });
    render(<AuditPage />);

    await waitFor(() => {
      expect(api.listActiveSessions).toHaveBeenCalledTimes(1);
    });
    const sessionsTab = screen.getByRole("tab", { name: /会话历史/ });
    expect(sessionsTab).toHaveTextContent("2");
    expect(api.listSessionHistory).not.toHaveBeenCalled();

    fireEvent.click(sessionsTab);

    expect(await screen.findByText("服务端超时")).toBeInTheDocument();
    expect(api.listSessionHistory).toHaveBeenCalledWith({
      offset: 0,
      limit: 50,
      tokenId: null,
      connectionId: null,
    });

    fireEvent.change(screen.getByLabelText("Token ID"), {
      target: { value: "tok-h" },
    });
    fireEvent.change(screen.getByLabelText("Connection ID"), {
      target: { value: "conn-h" },
    });
    fireEvent.click(screen.getByRole("button", { name: "查询" }));

    await waitFor(() => {
      expect(api.listSessionHistory).toHaveBeenLastCalledWith({
        offset: 0,
        limit: 50,
        tokenId: "tok-h",
        connectionId: "conn-h",
      });
    });
  });
});

describe("AuditPage 导出与错误", () => {
  it("导出成功后展示文件路径与行数", async () => {
    render(<AuditPage />);

    await waitFor(() => {
      expect(api.queryAudit).toHaveBeenCalled();
    });
    fireEvent.click(screen.getByRole("button", { name: "导出 CSV" }));

    const feedback = await screen.findByText(/已导出 3 行/);
    expect(feedback).toHaveTextContent("C:\\logs\\audit.csv");
    expect(api.exportAudit).toHaveBeenCalledTimes(1);
  });

  it("导出失败时用统一错误提示", async () => {
    api.exportAudit.mockRejectedValue({
      code: "not_implemented",
      message: "export_audit 尚未实现",
    });
    render(<AuditPage />);

    fireEvent.click(screen.getByRole("button", { name: "导出 CSV" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("后端命令尚未接通");
    expect(alert).toHaveTextContent("审计导出");
  });

  it("not_implemented 查询错误提示后端命令未接通", async () => {
    api.queryAudit.mockRejectedValue({
      code: "not_implemented",
      message: "query_audit 尚未实现",
    });
    render(<AuditPage />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("后端命令尚未接通");
    expect(alert).toHaveTextContent("审计查询");
    expect(screen.queryByText("没有匹配的审计记录")).not.toBeInTheDocument();
  });
});
