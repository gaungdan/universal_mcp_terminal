import { useEffect, useId, useState } from "react";
import type { ReactNode } from "react";
import type { ConfigField, ConfigSchema } from "../api/types";

export type SchemaFormProps = {
  schema: ConfigSchema;
  values: Record<string, unknown>;
  onChange: (values: Record<string, unknown>) => void;
  secrets: Record<string, string>;
  onSecretsChange: (secrets: Record<string, string>) => void;
  disabled?: boolean;
};

function looselyEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) {
    return true;
  }
  if (a === null || a === undefined || b === null || b === undefined) {
    return false;
  }
  if (typeof a === "object" || typeof b === "object") {
    return false;
  }
  return String(a) === String(b);
}

function isFieldVisible(field: ConfigField, values: Record<string, unknown>): boolean {
  const condition = field.visibleWhen;
  if (!condition) {
    return true;
  }
  return looselyEqual(values[condition.key], condition.equals);
}

function defaultValueFor(field: ConfigField): unknown {
  const raw = field.defaultValue;
  if (raw === undefined || raw === null) {
    return undefined;
  }
  switch (field.kind.kind) {
    case "boolean":
      if (typeof raw === "boolean") {
        return raw;
      }
      return raw === "true" ? true : raw === "false" ? false : Boolean(raw);
    case "number": {
      const parsed = typeof raw === "number" ? raw : Number(raw);
      return Number.isFinite(parsed) ? parsed : undefined;
    }
    default:
      if (typeof raw === "string") {
        return raw;
      }
      if (typeof raw === "number" || typeof raw === "boolean") {
        return String(raw);
      }
      return undefined;
  }
}

function normalizeForm(
  schema: ConfigSchema,
  values: Record<string, unknown>,
  secrets: Record<string, string>,
): { values: Record<string, unknown>; secrets: Record<string, string> } {
  let valuesChanged = false;
  let secretsChanged = false;
  const nextValues = { ...values };
  const nextSecrets = { ...secrets };

  for (const field of schema.fields) {
    if (!isFieldVisible(field, values)) {
      if (Object.hasOwn(nextValues, field.key)) {
        delete nextValues[field.key];
        valuesChanged = true;
      }
      if (Object.hasOwn(nextSecrets, field.key)) {
        delete nextSecrets[field.key];
        secretsChanged = true;
      }
      continue;
    }
    if (field.secret || nextValues[field.key] !== undefined) {
      continue;
    }
    const fallback = defaultValueFor(field);
    if (fallback === undefined) {
      continue;
    }
    nextValues[field.key] = fallback;
    valuesChanged = true;
  }

  return {
    values: valuesChanged ? nextValues : values,
    secrets: secretsChanged ? nextSecrets : secrets,
  };
}

function isEmptyValue(value: unknown): boolean {
  if (value === undefined || value === null) {
    return true;
  }
  if (typeof value === "string") {
    return value.trim().length === 0;
  }
  return false;
}

function validateField(field: ConfigField, value: unknown): string | null {
  if (isEmptyValue(value)) {
    return field.required ? `${field.label}为必填项` : null;
  }
  const rules = field.validation;
  if (!rules) {
    return null;
  }
  if (typeof value === "string") {
    if (rules.minLength !== undefined && value.length < rules.minLength) {
      return `${field.label}至少需要 ${rules.minLength} 个字符`;
    }
    if (rules.maxLength !== undefined && value.length > rules.maxLength) {
      return `${field.label}最多允许 ${rules.maxLength} 个字符`;
    }
    if (rules.pattern) {
      try {
        if (!new RegExp(rules.pattern).test(value)) {
          return `${field.label}格式不正确`;
        }
      } catch {
        // 非法正则交由后端校验，前端不阻断输入。
      }
    }
  }
  if (typeof value === "number") {
    if (rules.min !== undefined && value < rules.min) {
      return `${field.label}不能小于 ${rules.min}`;
    }
    if (rules.max !== undefined && value > rules.max) {
      return `${field.label}不能大于 ${rules.max}`;
    }
  }
  return null;
}

export default function SchemaForm({
  schema,
  values,
  onChange,
  secrets,
  onSecretsChange,
  disabled = false,
}: SchemaFormProps) {
  const formId = `schema-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}-${schema.protocolId}`;
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});

  useEffect(() => {
    const normalized = normalizeForm(schema, values, secrets);
    if (normalized.values !== values) {
      onChange(normalized.values);
    }
    if (normalized.secrets !== secrets) {
      onSecretsChange(normalized.secrets);
    }
  }, [schema, values, secrets, onChange, onSecretsChange]);

  const updateValue = (key: string, value: unknown) => {
    const next = { ...values };
    if (value === undefined) {
      delete next[key];
    } else {
      next[key] = value;
    }
    onChange(next);
  };

  const updateSecret = (key: string, value: string) => {
    const next = { ...secrets };
    if (value === "") {
      delete next[key];
    } else {
      next[key] = value;
    }
    onSecretsChange(next);
  };

  const markTouched = (key: string) => {
    setTouched((previous) => (previous[key] ? previous : { ...previous, [key]: true }));
  };

  const renderField = (field: ConfigField) => {
    const kind = field.kind;
    const inputId = `${formId}-${field.key}`;
    const value = field.secret ? (secrets[field.key] ?? "") : values[field.key];
    const error = validateField(field, value);
    const showError = error !== null && (touched[field.key] === true || !isEmptyValue(value));
    const describedBy = [
      field.help ? `${inputId}-help` : null,
      showError ? `${inputId}-error` : null,
    ]
      .filter(Boolean)
      .join(" ");

    const commit = (next: unknown) => {
      if (!field.secret) {
        updateValue(field.key, next);
        return;
      }
      if (typeof next === "string") {
        updateSecret(field.key, next);
      } else if (next === undefined) {
        updateSecret(field.key, "");
      } else {
        updateSecret(field.key, String(next));
      }
    };

    const base = {
      id: inputId,
      name: field.key,
      disabled,
      "aria-required": field.required || undefined,
      "aria-invalid": showError || undefined,
      "aria-describedby": describedBy || undefined,
      className: "form-field__control",
      onBlur: () => markTouched(field.key),
    };

    let control: ReactNode;

    switch (kind.kind) {
      case "boolean":
        control = (
          <input
            {...base}
            type="checkbox"
            className="form-field__checkbox"
            checked={value === true}
            onChange={(event) => commit(event.target.checked)}
          />
        );
        break;
      case "number":
        control = (
          <input
            {...base}
            type="number"
            inputMode="decimal"
            step="any"
            placeholder={field.placeholder}
            value={typeof value === "number" || typeof value === "string" ? String(value) : ""}
            onChange={(event) => {
              const raw = event.target.value;
              if (raw.trim() === "") {
                commit(undefined);
                return;
              }
              const parsed = Number(raw);
              if (Number.isFinite(parsed)) {
                commit(parsed);
              }
            }}
          />
        );
        break;
      case "select":
        control = (
          <select
            {...base}
            value={typeof value === "string" ? value : ""}
            onChange={(event) =>
              commit(event.target.value === "" ? undefined : event.target.value)
            }
          >
            <option value="">{field.placeholder ?? "请选择"}</option>
            {kind.options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        );
        break;
      case "multiline":
        control = (
          <textarea
            {...base}
            rows={4}
            placeholder={field.placeholder}
            value={typeof value === "string" ? value : ""}
            onChange={(event) => commit(event.target.value)}
          />
        );
        break;
      default: {
        const isPassword = kind.kind === "password" || field.secret;
        const visible = revealed[field.key] === true;
        const input = (
          <input
            {...base}
            type={isPassword && !visible ? "password" : "text"}
            placeholder={field.placeholder}
            autoComplete={isPassword ? "new-password" : "off"}
            spellCheck={kind.kind !== "password"}
            value={
              typeof value === "string"
                ? value
                : typeof value === "number"
                  ? String(value)
                  : ""
            }
            onChange={(event) => commit(event.target.value)}
          />
        );
        control = isPassword ? (
          <div className="form-field__row">
            {input}
            <button
              type="button"
              className="btn btn--ghost btn--small"
              aria-pressed={visible}
              aria-label={`${visible ? "隐藏" : "显示"}${field.label}`}
              disabled={disabled}
              onClick={() =>
                setRevealed((previous) => ({ ...previous, [field.key]: !visible }))
              }
            >
              {visible ? "隐藏" : "显示"}
            </button>
          </div>
        ) : (
          input
        );
        break;
      }
    }

    return (
      <div
        key={field.key}
        className={`form-field${showError ? " form-field--invalid" : ""}`}
      >
        <label
          className={`form-field__label${field.required ? " form-field__label--required" : ""}`}
          htmlFor={inputId}
        >
          {field.label}
        </label>
        {control}
        {field.help ? (
          <p className="form-field__help" id={`${inputId}-help`}>
            {field.help}
          </p>
        ) : null}
        {showError && error ? (
          <p className="form-field__error" id={`${inputId}-error`} role="alert">
            {error}
          </p>
        ) : null}
      </div>
    );
  };

  const visibleFields = schema.fields.filter((field) => isFieldVisible(field, values));

  return (
    <div className="schema-form">
      {visibleFields.length === 0 ? (
        <p className="schema-form__empty">当前协议没有可配置字段。</p>
      ) : (
        visibleFields.map((field) => renderField(field))
      )}
    </div>
  );
}
