import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ConfigField, ConfigSchema } from "../api/types";
import SchemaForm from "../components/SchemaForm";

function field(partial: Partial<ConfigField> & { key: string; label: string }): ConfigField {
  return {
    kind: { kind: "text" },
    required: false,
    secret: false,
    ...partial,
  };
}

function makeSchema(...fields: ConfigField[]): ConfigSchema {
  return { protocolId: "test-protocol", fields };
}

type HarnessProps = {
  schema: ConfigSchema;
  initialValues?: Record<string, unknown>;
  initialSecrets?: Record<string, string>;
  onChangeSpy?: (values: Record<string, unknown>) => void;
};

function Harness({
  schema,
  initialValues = {},
  initialSecrets = {},
  onChangeSpy,
}: HarnessProps) {
  const [values, setValues] = useState(initialValues);
  const [secrets, setSecrets] = useState(initialSecrets);

  return (
    <div>
      <SchemaForm
        schema={schema}
        values={values}
        secrets={secrets}
        onChange={(next) => {
          onChangeSpy?.(next);
          setValues(next);
        }}
        onSecretsChange={setSecrets}
      />
      <pre data-testid="values">{JSON.stringify(values)}</pre>
      <pre data-testid="secrets">{JSON.stringify(secrets)}</pre>
    </div>
  );
}

function readJson(testId: string): Record<string, unknown> {
  return JSON.parse(screen.getByTestId(testId).textContent ?? "{}") as Record<
    string,
    unknown
  >;
}

describe("SchemaForm", () => {
  it("按 schema 渲染全部 7 种字段类型", () => {
    render(
      <Harness
        schema={makeSchema(
          field({ key: "host", label: "主机" }),
          field({ key: "password", label: "密码", kind: { kind: "password" } }),
          field({ key: "port", label: "端口", kind: { kind: "number" } }),
          field({ key: "tls", label: "启用 TLS", kind: { kind: "boolean" } }),
          field({
            key: "mode",
            label: "模式",
            kind: {
              kind: "select",
              options: [
                { value: "a", label: "模式 A" },
                { value: "b", label: "模式 B" },
              ],
            },
          }),
          field({ key: "script", label: "初始化脚本", kind: { kind: "multiline" } }),
          field({ key: "dir", label: "工作目录", kind: { kind: "path" } }),
        )}
      />,
    );

    expect(screen.getByLabelText("主机")).toHaveAttribute("type", "text");
    expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("端口")).toHaveAttribute("type", "number");
    expect(screen.getByLabelText("启用 TLS")).toHaveAttribute("type", "checkbox");
    expect(screen.getByRole("combobox", { name: "模式" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "初始化脚本" })).toBeInTheDocument();
    expect(screen.getByLabelText("工作目录")).toHaveAttribute("type", "text");
  });

  it("必填字段失焦后给出 role=alert 提示", () => {
    render(<Harness schema={makeSchema(field({ key: "name", label: "名称", required: true }))} />);

    const input = screen.getByLabelText("名称");
    expect(input).toHaveAttribute("aria-required", "true");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.blur(input);
    expect(screen.getByRole("alert")).toHaveTextContent("名称为必填项");
  });

  it("visibleWhen 控制显隐，隐藏时清空值", () => {
    render(
      <Harness
        initialValues={{ mode: "a", extra: "keep-me" }}
        schema={makeSchema(
          field({
            key: "mode",
            label: "模式",
            kind: {
              kind: "select",
              options: [
                { value: "a", label: "模式 A" },
                { value: "b", label: "模式 B" },
              ],
            },
          }),
          field({
            key: "extra",
            label: "附加项",
            visibleWhen: { key: "mode", equals: "b" },
          }),
        )}
      />,
    );

    expect(screen.queryByLabelText("附加项")).not.toBeInTheDocument();
    expect(readJson("values")).toEqual({ mode: "a" });

    fireEvent.change(screen.getByRole("combobox", { name: "模式" }), {
      target: { value: "b" },
    });

    expect(screen.getByLabelText("附加项")).toBeInTheDocument();
    expect(readJson("values")).toEqual({ mode: "b" });
  });

  it("数字字段输出 number 而不是字符串", () => {
    render(
      <Harness
        schema={makeSchema(
          field({ key: "port", label: "端口", kind: { kind: "number" } }),
        )}
      />,
    );

    fireEvent.change(screen.getByLabelText("端口"), { target: { value: "2222" } });

    const values = readJson("values");
    expect(values).toEqual({ port: 2222 });
    expect(typeof values.port).toBe("number");
  });

  it("secret 字段只写入 secrets，与普通字段互不污染", () => {
    render(
      <Harness
        schema={makeSchema(
          field({ key: "host", label: "主机" }),
          field({
            key: "password",
            label: "密码",
            kind: { kind: "password" },
            secret: true,
          }),
        )}
      />,
    );

    fireEvent.change(screen.getByLabelText("主机"), {
      target: { value: "127.0.0.1" },
    });
    fireEvent.change(screen.getByLabelText("密码"), { target: { value: "s3cret" } });

    expect(readJson("values")).toEqual({ host: "127.0.0.1" });
    expect(readJson("secrets")).toEqual({ password: "s3cret" });
  });

  it("secret 字段支持显示/隐藏切换", () => {
    render(
      <Harness
        schema={makeSchema(
          field({
            key: "password",
            label: "密码",
            kind: { kind: "password" },
            secret: true,
          }),
        )}
      />,
    );

    expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");

    fireEvent.click(screen.getByRole("button", { name: "显示密码" }));
    expect(screen.getByLabelText("密码")).toHaveAttribute("type", "text");

    fireEvent.click(screen.getByRole("button", { name: "隐藏密码" }));
    expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");
  });

  it("字段首次出现时填充 defaultValue", () => {
    render(
      <Harness
        schema={makeSchema(
          field({
            key: "port",
            label: "端口",
            kind: { kind: "number" },
            defaultValue: 22,
          }),
          field({
            key: "tls",
            label: "启用 TLS",
            kind: { kind: "boolean" },
            defaultValue: true,
          }),
          field({ key: "shell", label: "Shell", defaultValue: "/bin/sh" }),
        )}
      />,
    );

    expect(readJson("values")).toEqual({ port: 22, tls: true, shell: "/bin/sh" });
  });

  it("validation 规则在前端给出提示", () => {
    render(
      <Harness
        schema={makeSchema(
          field({
            key: "code",
            label: "编码",
            required: true,
            validation: { minLength: 3, maxLength: 8, pattern: "^[a-z]+$" },
          }),
        )}
      />,
    );

    const input = screen.getByLabelText("编码");
    fireEvent.change(input, { target: { value: "ab" } });
    expect(screen.getByRole("alert")).toHaveTextContent("编码至少需要 3 个字符");

    fireEvent.change(input, { target: { value: "ABC" } });
    expect(screen.getByRole("alert")).toHaveTextContent("编码格式不正确");

    fireEvent.change(input, { target: { value: "abcdefghi" } });
    expect(screen.getByRole("alert")).toHaveTextContent("编码最多允许 8 个字符");

    fireEvent.change(input, { target: { value: "abc" } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("受控 onChange 收到新对象且不修改原 values", () => {
    const onChangeSpy = vi.fn();
    const initialValues = { host: "a" };
    render(
      <Harness
        schema={makeSchema(field({ key: "host", label: "主机" }))}
        initialValues={initialValues}
        onChangeSpy={onChangeSpy}
      />,
    );

    fireEvent.change(screen.getByLabelText("主机"), { target: { value: "b" } });

    const lastCall = onChangeSpy.mock.calls.at(-1)?.[0] as Record<string, unknown>;
    expect(lastCall).toEqual({ host: "b" });
    expect(initialValues).toEqual({ host: "a" });
  });
});
