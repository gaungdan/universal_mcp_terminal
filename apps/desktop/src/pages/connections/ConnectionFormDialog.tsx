import { useEffect, useId, useState } from "react";
import type { FormEvent, MouseEvent } from "react";
import { createPortal } from "react-dom";
import {
  createConnection,
  getConnection,
  testConnectionDraft,
  updateConnection,
} from "../../api/client";
import type {
  ConnectionDto,
  ConnectionTestResult,
  ProtocolInfoDto,
} from "../../api/types";
import SchemaForm from "../../components/SchemaForm";
import { toUserMessage } from "../../lib/errors";
import TestResultCard from "./TestResultCard";
import { CONFLICT_MESSAGE, isConflictError, parseTags, toSecretInputs } from "./helpers";

export type ConnectionFormDialogProps = {
  mode: "create" | "edit";
  connectionId?: string;
  protocols: ProtocolInfoDto[];
  onClose: () => void;
  onSaved: (connection: ConnectionDto) => void;
};

const errorTextStyle = { margin: 0, fontSize: 13, color: "var(--color-danger)" };

function trimmedOrNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export default function ConnectionFormDialog({
  mode,
  connectionId,
  protocols,
  onClose,
  onSaved,
}: ConnectionFormDialogProps) {
  const rawId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const titleId = `connection-form-title-${rawId}`;
  const fieldId = (name: string) => `connection-form-${rawId}-${name}`;

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [tags, setTags] = useState("");
  const [notes, setNotes] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [protocolId, setProtocolId] = useState("");
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [detail, setDetail] = useState<ConnectionDto | null>(null);
  const [secretFieldsSet, setSecretFieldsSet] = useState<string[]>([]);
  const [loading, setLoading] = useState(mode === "edit");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);

  useEffect(() => {
    if (mode !== "edit" || !connectionId) {
      return;
    }
    let cancelled = false;
    void getConnection(connectionId).then(
      (loaded) => {
        if (cancelled) {
          return;
        }
        setDetail(loaded.connection);
        setName(loaded.connection.name);
        setDescription(loaded.connection.description ?? "");
        setTags(loaded.connection.tags.join(", "));
        setNotes(loaded.connection.notes ?? "");
        setEnabled(loaded.connection.enabled);
        setProtocolId(loaded.connection.protocolId);
        setValues(loaded.config);
        setSecrets({});
        setSecretFieldsSet(loaded.secretFieldsSet);
        setLoading(false);
      },
      (error: unknown) => {
        if (cancelled) {
          return;
        }
        setLoadError(toUserMessage(error));
        setLoading(false);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [mode, connectionId]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !submitting) {
        onClose();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, submitting]);

  const selectedProtocol =
    protocols.find((item) => item.protocolId === protocolId) ?? null;
  const schema = selectedProtocol?.configSchema ?? null;

  const handleProtocolChange = (next: string) => {
    setProtocolId(next);
    setValues({});
    setSecrets({});
    setTestResult(null);
    setTestError(null);
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSubmitError(null);
    if (submitting || loading) {
      return;
    }
    const trimmedName = name.trim();
    if (trimmedName === "") {
      setSubmitError("请填写连接名称。");
      return;
    }
    if (protocolId === "") {
      setSubmitError("请选择协议类型。");
      return;
    }
    if (mode === "edit" && !detail) {
      setSubmitError("连接详情尚未加载完成，请稍后重试。");
      return;
    }

    const secretInputs = toSecretInputs(secrets);
    const common = {
      name: trimmedName,
      description: trimmedOrNull(description),
      tags: parseTags(tags),
      notes: trimmedOrNull(notes),
      enabled,
      config: values,
    };

    setSubmitting(true);
    try {
      let saved: ConnectionDto;
      if (mode === "edit" && detail) {
        saved = await updateConnection({
          id: detail.id,
          ...common,
          secrets: secretInputs.length === 0 ? null : secretInputs,
          expectedRevision: detail.revision,
        });
      } else {
        saved = await createConnection({
          ...common,
          protocolId,
          secrets: secretInputs,
        });
      }
      onSaved(saved);
    } catch (error) {
      setSubmitError(isConflictError(error) ? CONFLICT_MESSAGE : toUserMessage(error));
    } finally {
      setSubmitting(false);
    }
  };

  const handleTest = async () => {
    setTestError(null);
    setTestResult(null);
    if (protocolId === "") {
      setTestError("请先选择协议类型。");
      return;
    }
    setTesting(true);
    try {
      const result = await testConnectionDraft({
        protocolId,
        config: values,
        secrets: toSecretInputs(secrets),
      });
      setTestResult(result);
    } catch (error) {
      setTestError(toUserMessage(error));
    } finally {
      setTesting(false);
    }
  };

  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget && !submitting) {
      onClose();
    }
  };

  const isEdit = mode === "edit";

  return createPortal(
    <div className="dialog-backdrop" onMouseDown={handleBackdropMouseDown}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        style={{ maxWidth: 760, maxHeight: "calc(100vh - 48px)", overflowY: "auto" }}
      >
        <h2 className="dialog__title" id={titleId}>
          {isEdit ? "编辑连接" : "新建连接"}
        </h2>
        <form onSubmit={(event) => void handleSubmit(event)} noValidate>
          {loadError ? (
            <p role="alert" style={errorTextStyle}>
              {loadError}
            </p>
          ) : null}
          {loading ? (
            <p role="status" style={{ margin: 0, color: "var(--color-muted)" }}>
              正在加载连接详情…
            </p>
          ) : null}

          <div className="form-field">
            <label
              className="form-field__label form-field__label--required"
              htmlFor={fieldId("name")}
            >
              名称
            </label>
            <input
              id={fieldId("name")}
              className="form-field__control"
              value={name}
              maxLength={80}
              autoFocus
              placeholder="例如：生产跳板机"
              onChange={(event) => setName(event.target.value)}
            />
          </div>

          <div className="form-field" style={{ marginTop: 14 }}>
            <label
              className="form-field__label form-field__label--required"
              htmlFor={fieldId("protocol")}
            >
              协议
            </label>
            <select
              id={fieldId("protocol")}
              className="form-field__control"
              value={protocolId}
              disabled={isEdit || loading}
              onChange={(event) => handleProtocolChange(event.target.value)}
            >
              <option value="">请选择协议</option>
              {protocols.map((item) => (
                <option key={item.protocolId} value={item.protocolId}>
                  {item.displayName}
                </option>
              ))}
            </select>
            {selectedProtocol?.description ? (
              <p className="form-field__help">{selectedProtocol.description}</p>
            ) : null}
            {isEdit ? (
              <p className="form-field__help">协议在创建后不可修改。</p>
            ) : null}
          </div>

          <div className="form-field" style={{ marginTop: 14 }}>
            <label className="form-field__label" htmlFor={fieldId("description")}>
              连接描述
            </label>
            <input
              id={fieldId("description")}
              className="form-field__control"
              value={description}
              maxLength={160}
              placeholder="简述该连接的用途"
              onChange={(event) => setDescription(event.target.value)}
            />
          </div>

          <div className="form-field" style={{ marginTop: 14 }}>
            <label className="form-field__label" htmlFor={fieldId("tags")}>
              标签
            </label>
            <input
              id={fieldId("tags")}
              className="form-field__control"
              value={tags}
              placeholder="例如：production, ops"
              onChange={(event) => setTags(event.target.value)}
            />
            <p className="form-field__help">多个标签用逗号分隔，用于搜索与分类。</p>
          </div>

          <div className="form-field" style={{ marginTop: 14 }}>
            <label className="form-field__label" htmlFor={fieldId("notes")}>
              备注
            </label>
            <textarea
              id={fieldId("notes")}
              className="form-field__control"
              rows={3}
              value={notes}
              onChange={(event) => setNotes(event.target.value)}
            />
          </div>

          <div className="form-field" style={{ marginTop: 14 }}>
            <label className="form-field__label" htmlFor={fieldId("enabled")}>
              启用连接
            </label>
            <input
              id={fieldId("enabled")}
              type="checkbox"
              className="form-field__checkbox"
              checked={enabled}
              onChange={(event) => setEnabled(event.target.checked)}
            />
            <p className="form-field__help">
              停用后 MCP 不会再使用该连接创建新的 Session。
            </p>
          </div>

          <hr
            style={{
              margin: "20px 0 0",
              border: "none",
              borderTop: "1px solid var(--color-border)",
            }}
          />

          <h3 style={{ margin: "16px 0 12px", fontSize: 14, fontWeight: 600 }}>协议配置</h3>
          {isEdit && secretFieldsSet.length > 0 ? (
            <p className="form-field__help" style={{ margin: "0 0 12px" }}>
              敏感字段已加密保存，留空表示不修改。
            </p>
          ) : null}
          {schema ? (
            <SchemaForm
              schema={schema}
              values={values}
              onChange={setValues}
              secrets={secrets}
              onSecretsChange={setSecrets}
              disabled={loading || submitting}
            />
          ) : (
            <p className="schema-form__empty">
              {protocolId === ""
                ? "请选择协议类型以加载配置项。"
                : "未找到该协议的配置定义。"}
            </p>
          )}

          {submitError ? (
            <p role="alert" style={{ ...errorTextStyle, marginTop: 12 }}>
              {submitError}
            </p>
          ) : null}
          {testError ? (
            <p role="alert" style={{ ...errorTextStyle, marginTop: 12 }}>
              {testError}
            </p>
          ) : null}
          {testResult ? <TestResultCard result={testResult} /> : null}

          <div className="dialog__actions" style={{ justifyContent: "space-between" }}>
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => void handleTest()}
              disabled={testing || submitting || loading}
            >
              {testing ? "测试中…" : "测试连接"}
            </button>
            <div style={{ display: "flex", gap: 8 }}>
              <button
                type="button"
                className="btn btn--ghost"
                onClick={onClose}
                disabled={submitting}
              >
                取消
              </button>
              <button
                type="submit"
                className="btn btn--primary"
                disabled={submitting || loading}
              >
                {submitting ? "保存中…" : isEdit ? "保存修改" : "创建连接"}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
}
