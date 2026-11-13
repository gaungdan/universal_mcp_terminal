export type StatusTone = "success" | "warning" | "danger" | "info" | "neutral";

type StatusMeta = {
  label: string;
  tone: StatusTone;
};

const STATUS_META: Record<string, StatusMeta> = {
  enabled: { label: "已启用", tone: "success" },
  disabled: { label: "已停用", tone: "neutral" },
  active: { label: "生效中", tone: "success" },
  revoked: { label: "已撤销", tone: "danger" },
  connecting: { label: "连接中", tone: "info" },
  connected: { label: "已连接", tone: "success" },
  running: { label: "运行中", tone: "success" },
  closed: { label: "已关闭", tone: "neutral" },
  failed: { label: "失败", tone: "danger" },
  interrupted: { label: "已中断", tone: "warning" },
  passed: { label: "通过", tone: "success" },
  skipped: { label: "跳过", tone: "neutral" },
  allowed: { label: "允许", tone: "success" },
  denied: { label: "拒绝", tone: "danger" },
  succeeded: { label: "成功", tone: "success" },
  timed_out: { label: "超时", tone: "warning" },
  sent: { label: "已发送", tone: "info" },
  unknown: { label: "未知", tone: "neutral" },
};

export type StatusBadgeProps = {
  status: string;
  label?: string;
  className?: string;
};

export default function StatusBadge({ status, label, className }: StatusBadgeProps) {
  const meta = STATUS_META[status];
  const tone = meta ? meta.tone : "neutral";
  const text = label ?? (meta ? meta.label : status);

  return (
    <span className={`status-badge status-badge--${tone}${className ? ` ${className}` : ""}`}>
      <span className="status-badge__dot" aria-hidden="true" />
      {text}
    </span>
  );
}
