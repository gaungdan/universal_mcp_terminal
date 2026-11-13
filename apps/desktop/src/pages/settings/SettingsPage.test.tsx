import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { McpStatusDto, SettingsDto } from "../../api/types";
import SettingsPage from "./SettingsPage";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateMcpSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

vi.mock("../../api/client", () => api);

const ENDPOINT = "http://127.0.0.1:17321/mcp";

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

function makeSettings(overrides: Partial<SettingsDto> = {}): SettingsDto {
  return {
    mcp: makeMcp(),
    security: {
      defaultExecutionMode: "Restricted",
      sessionTimeoutSecs: 600,
      requireConfirmationForHighRisk: true,
    },
    logLevel: "info",
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getSettings.mockResolvedValue(makeSettings());
  api.updateMcpSettings.mockResolvedValue(makeMcp());
  api.updateSettings.mockImplementation(
    async (input: { security: SettingsDto["security"]; logLevel: string }) =>
      makeSettings({
        security: input.security,
        logLevel: input.logLevel,
      }),
  );
});

describe("系统设置 - MCP 配置", () => {
  it("端口越界时拦截保存", async () => {
    render(<SettingsPage />);

    fireEvent.change(await screen.findByLabelText("端口"), {
      target: { value: "70000" },
    });

    expect(
      await screen.findByText("端口必须是 1-65535 之间的整数"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存 MCP 配置" }));

    expect(api.updateMcpSettings).not.toHaveBeenCalled();
  });

  it("保存时提交正确的监听地址与端口", async () => {
    render(<SettingsPage />);

    fireEvent.change(await screen.findByLabelText("监听地址"), {
      target: { value: "127.0.0.2" },
    });
    fireEvent.change(screen.getByLabelText("端口"), {
      target: { value: "18080" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存 MCP 配置" }));

    await waitFor(() =>
      expect(api.updateMcpSettings).toHaveBeenCalledWith({
        host: "127.0.0.2",
        port: 18080,
      }),
    );
    expect(await screen.findByText("MCP 配置已保存。")).toBeInTheDocument();
  });

  it("服务运行中时提示需重启生效", async () => {
    api.getSettings.mockResolvedValue(
      makeSettings({ mcp: makeMcp({ running: true }) }),
    );

    render(<SettingsPage />);

    expect(
      await screen.findByText(/修改监听地址或端口后需重启服务才能生效/),
    ).toBeInTheDocument();
    expect(screen.getByText(ENDPOINT)).toBeInTheDocument();
    expect(screen.getByText("运行中")).toBeInTheDocument();
  });

  it("保存失败时展示友好错误", async () => {
    api.updateMcpSettings.mockRejectedValue({
      code: "validation",
      message: "监听地址格式不合法",
    });

    render(<SettingsPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "保存 MCP 配置" }),
    );

    const alert = await screen.findByText(/监听地址格式不合法/);
    expect(alert).toHaveTextContent("提交的内容不合法");
  });
});

describe("系统设置 - 安全与日志", () => {
  it("保存安全设置时提交执行模式、超时、确认开关与日志级别", async () => {
    render(<SettingsPage />);

    fireEvent.change(await screen.findByLabelText("默认执行模式"), {
      target: { value: "FullAccess" },
    });
    fireEvent.change(screen.getByLabelText("Session 超时（秒）"), {
      target: { value: "900" },
    });
    fireEvent.click(screen.getByLabelText("高风险操作需二次确认"));
    fireEvent.change(screen.getByLabelText("日志级别"), {
      target: { value: "debug" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存安全设置" }));

    await waitFor(() =>
      expect(api.updateSettings).toHaveBeenCalledWith({
        security: {
          defaultExecutionMode: "FullAccess",
          sessionTimeoutSecs: 900,
          requireConfirmationForHighRisk: false,
        },
        logLevel: "debug",
      }),
    );
    expect(await screen.findByText("安全设置已保存。")).toBeInTheDocument();
  });

  it("Session 超时非正数时拦截保存", async () => {
    render(<SettingsPage />);

    fireEvent.change(await screen.findByLabelText("Session 超时（秒）"), {
      target: { value: "0" },
    });

    expect(
      await screen.findByText("Session 超时时间必须是大于 0 的整数"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存安全设置" }));

    expect(api.updateSettings).not.toHaveBeenCalled();
  });
});
