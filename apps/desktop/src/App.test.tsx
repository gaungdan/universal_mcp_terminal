import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import App from "./App";

describe("App", () => {
  it("渲染五个侧边栏导航项", () => {
    render(<App />);

    const nav = screen.getByRole("navigation");
    for (const label of [
      "首页",
      "连接管理",
      "访问控制",
      "审计日志",
      "系统设置",
    ]) {
      expect(within(nav).getByRole("link", { name: label })).toBeInTheDocument();
    }
  });
});
