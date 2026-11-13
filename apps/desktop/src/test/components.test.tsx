import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ConfirmDialog from "../components/ConfirmDialog";
import EmptyState from "../components/EmptyState";
import SectionCard from "../components/SectionCard";
import StatusBadge from "../components/StatusBadge";
import Toolbar from "../components/Toolbar";

describe("StatusBadge", () => {
  it("已知状态输出中文标签与语义色", () => {
    render(<StatusBadge status="enabled" />);
    const badge = screen.getByText("已启用");
    expect(badge).toHaveClass("status-badge--success");

    render(<StatusBadge status="revoked" />);
    expect(screen.getByText("已撤销")).toHaveClass("status-badge--danger");
  });

  it("未知状态回退到原始文本与中性色", () => {
    render(<StatusBadge status="pending_review" />);
    expect(screen.getByText("pending_review")).toHaveClass("status-badge--neutral");
  });
});

describe("EmptyState / SectionCard / Toolbar", () => {
  it("组合渲染标题、描述与操作区", () => {
    render(
      <SectionCard
        title="连接列表"
        description="管理全部 MCP 连接"
        actions={<button type="button">新建</button>}
      >
        <Toolbar actions={<button type="button">刷新</button>}>
          <span>共 0 条</span>
        </Toolbar>
        <EmptyState
          title="暂无连接"
          description="创建第一个连接后即可开始使用。"
          action={<button type="button">创建连接</button>}
        />
      </SectionCard>,
    );

    expect(screen.getByRole("heading", { name: "连接列表" })).toBeInTheDocument();
    expect(screen.getByText("管理全部 MCP 连接")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新" })).toBeInTheDocument();
    expect(screen.getByText("暂无连接")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接" })).toBeInTheDocument();
  });
});

describe("ConfirmDialog", () => {
  function renderDialog(open: boolean) {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const view = render(
      <ConfirmDialog
        open={open}
        title="删除连接"
        description="删除后无法恢复。"
        confirmText="删除"
        tone="danger"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    return { onConfirm, onCancel, ...view };
  }

  it("关闭时不渲染对话框", () => {
    renderDialog(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("打开时聚焦确认按钮，Esc 触发取消", () => {
    const { onCancel, onConfirm } = renderDialog(true);

    const dialog = screen.getByRole("dialog", { name: "删除连接" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("button", { name: "删除" })).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("点击确认按钮触发 onConfirm", () => {
    const { onConfirm } = renderDialog(true);
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});
