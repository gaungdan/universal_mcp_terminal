import { describe, expect, it } from "vitest";
import { formatUnixMillis, isApiError, toUserMessage } from "../lib/errors";

describe("isApiError", () => {
  it("识别带 code 与 message 的错误对象", () => {
    expect(isApiError({ code: "not_found", message: "不存在" })).toBe(true);
    expect(isApiError({ code: "not_found" })).toBe(false);
    expect(isApiError(new Error("boom"))).toBe(false);
    expect(isApiError("boom")).toBe(false);
    expect(isApiError(null)).toBe(false);
    expect(isApiError(undefined)).toBe(false);
  });
});

describe("toUserMessage", () => {
  it("按 code 输出中文提示", () => {
    expect(toUserMessage({ code: "access_denied", message: "" })).toBe(
      "没有权限执行此操作。",
    );
    expect(toUserMessage({ code: "validation", message: "name 不能为空" })).toBe(
      "提交的内容不合法，请检查后重试。（name 不能为空）",
    );
    expect(toUserMessage({ code: "not_found", message: "id=1" })).toContain("不存在");
    expect(toUserMessage({ code: "conflict", message: "revision" })).toContain("刷新");
    expect(toUserMessage({ code: "not_implemented", message: "" })).toContain(
      "尚未实现",
    );
  });

  it("未知错误给出兜底提示且不泄露堆栈", () => {
    const message = toUserMessage(new Error("boom\n    at doThing (app.js:1:1)"));
    expect(message).toBe("操作失败，请稍后重试。（boom）");
    expect(message).not.toContain("doThing");
    expect(toUserMessage(null)).toBe("操作失败，请稍后重试。");
    expect(toUserMessage({})).toBe("操作失败，请稍后重试。");
  });
});

describe("formatUnixMillis", () => {
  it("按本地时间格式化", () => {
    const ms = new Date(2026, 0, 2, 3, 4, 5).getTime();
    expect(formatUnixMillis(ms)).toBe("2026-01-02 03:04:05");
  });

  it("空值返回破折号", () => {
    expect(formatUnixMillis()).toBe("—");
    expect(formatUnixMillis(null)).toBe("—");
    expect(formatUnixMillis(Number.NaN)).toBe("—");
  });
});
