import { useEffect, useId, useRef, useState } from "react";
import type { FormEvent, MouseEvent } from "react";
import { createPortal } from "react-dom";
import type { CreateTokenInput } from "../../api/types";

export type CreateTokenDialogProps = {
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onSubmit: (input: CreateTokenInput) => void;
};

export default function CreateTokenDialog({
  busy,
  error,
  onCancel,
  onSubmit,
}: CreateTokenDialogProps) {
  const rawId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const titleId = `create-token-title-${rawId}`;
  const descriptionId = `create-token-description-${rawId}`;
  const nameId = `create-token-name-${rawId}`;
  const detailId = `create-token-detail-${rawId}`;
  const expiresId = `create-token-expires-${rawId}`;

  const nameRef = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);

  useEffect(() => {
    nameRef.current?.focus();
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) {
        event.preventDefault();
        onCancel();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [busy, onCancel]);

  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget && !busy) {
      onCancel();
    }
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedName = name.trim();
    if (trimmedName.length === 0) {
      setLocalError("请输入 Token 名称。");
      return;
    }

    let expiresAtMillis: number | null = null;
    if (expiresAt.trim().length > 0) {
      const parsed = new Date(expiresAt).getTime();
      if (!Number.isFinite(parsed)) {
        setLocalError("有效期格式不正确，请重新选择。");
        return;
      }
      expiresAtMillis = parsed;
    }

    setLocalError(null);
    onSubmit({
      name: trimmedName,
      description: description.trim().length > 0 ? description.trim() : null,
      expiresAt: expiresAtMillis,
    });
  };

  return createPortal(
    <div className="dialog-backdrop" onMouseDown={handleBackdropMouseDown}>
      <div
        className="dialog token-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
      >
        <h2 className="dialog__title" id={titleId}>
          新建 Token
        </h2>
        <p className="dialog__description" id={descriptionId}>
          Token 明文只会在创建成功后显示一次，请提前准备好保存位置。
        </p>
        <form className="token-form" onSubmit={handleSubmit} noValidate>
          <div className="form-field">
            <label
              className="form-field__label form-field__label--required"
              htmlFor={nameId}
            >
              名称
            </label>
            <input
              ref={nameRef}
              id={nameId}
              className="form-field__control"
              type="text"
              value={name}
              maxLength={120}
              placeholder="例如：Claude Code"
              disabled={busy}
              onChange={(event) => setName(event.target.value)}
            />
          </div>

          <div className="form-field">
            <label className="form-field__label" htmlFor={detailId}>
              描述
            </label>
            <textarea
              id={detailId}
              className="form-field__control"
              rows={3}
              value={description}
              maxLength={500}
              placeholder="可选，用于说明该 Token 的用途"
              disabled={busy}
              onChange={(event) => setDescription(event.target.value)}
            />
          </div>

          <div className="form-field">
            <label className="form-field__label" htmlFor={expiresId}>
              有效期（可选）
            </label>
            <input
              id={expiresId}
              className="form-field__control"
              type="datetime-local"
              value={expiresAt}
              disabled={busy}
              onChange={(event) => setExpiresAt(event.target.value)}
            />
            <p className="form-field__help">留空表示永久有效，时间按本地时区解释。</p>
          </div>

          {localError ? (
            <p className="form-field__error" role="alert">
              {localError}
            </p>
          ) : null}
          {error ? (
            <p className="form-field__error" role="alert">
              {error}
            </p>
          ) : null}

          <div className="dialog__actions">
            <button
              type="button"
              className="btn btn--ghost"
              onClick={onCancel}
              disabled={busy}
            >
              取消
            </button>
            <button type="submit" className="btn btn--primary" disabled={busy}>
              {busy ? "创建中…" : "创建 Token"}
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
}
