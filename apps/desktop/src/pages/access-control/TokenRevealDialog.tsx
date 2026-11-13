import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { TokenDto } from "../../api/types";

type CopyStatus = "idle" | "copied" | "failed";

export type TokenRevealDialogProps = {
  token: TokenDto;
  plaintext: string;
  onClose: () => void;
};

export default function TokenRevealDialog({
  token,
  plaintext,
  onClose,
}: TokenRevealDialogProps) {
  const rawId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const titleId = `token-reveal-title-${rawId}`;
  const warningId = `token-reveal-warning-${rawId}`;
  const copyRef = useRef<HTMLButtonElement>(null);
  const [copyStatus, setCopyStatus] = useState<CopyStatus>("idle");

  useEffect(() => {
    copyRef.current?.focus();
  }, []);

  const handleCopy = async () => {
    const clipboard = navigator.clipboard;
    if (!clipboard || typeof clipboard.writeText !== "function") {
      setCopyStatus("failed");
      return;
    }
    try {
      await clipboard.writeText(plaintext);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
    }
  };

  return createPortal(
    <div className="dialog-backdrop">
      <div
        className="dialog token-reveal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={warningId}
      >
        <h2 className="dialog__title" id={titleId}>
          Token 已创建
        </h2>
        <p className="token-reveal__warning" id={warningId} role="alert">
          此 Token 仅显示一次，请立即保存
        </p>
        <p className="token-reveal__name">{token.name}</p>
        <div className="token-reveal__value">
          <code data-testid="token-plaintext">{plaintext}</code>
        </div>
        <div className="token-reveal__actions">
          <button
            ref={copyRef}
            type="button"
            className="btn btn--primary"
            onClick={() => void handleCopy()}
          >
            复制 Token
          </button>
          {copyStatus === "copied" ? (
            <span className="token-reveal__feedback" role="status">
              已复制到剪贴板
            </span>
          ) : null}
          {copyStatus === "failed" ? (
            <span className="token-reveal__feedback token-reveal__feedback--error" role="alert">
              复制失败，请手动选中 Token 后复制
            </span>
          ) : null}
        </div>
        <p className="token-reveal__hint">
          关闭后无法再次查看完整 Token，列表中仅保留前缀
          <code>umt_{token.prefix}_••••••</code>。
        </p>
        <div className="dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            我已保存，关闭
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
