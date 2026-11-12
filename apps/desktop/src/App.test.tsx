import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "./App";

describe("App", () => {
  it("渲染五个侧边栏导航项", () => {
    render(<App />);

    for (const label of [
      "首页",
      "连接管理",
      "访问控制",
      "审计日志",
      "系统设置",
    ]) {
      expect(screen.getByRole("link", { name: label })).toBeInTheDocument();
    }
  });
});
