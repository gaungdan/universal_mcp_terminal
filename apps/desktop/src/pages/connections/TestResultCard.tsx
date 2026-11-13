import type { ConnectionTestResult } from "../../api/types";
import StatusBadge from "../../components/StatusBadge";

export type TestResultCardProps = {
  result: ConnectionTestResult;
};

export default function TestResultCard({ result }: TestResultCardProps) {
  const palette = result.ok
    ? { border: "var(--color-accent)", background: "var(--color-accent-soft)", text: "var(--color-accent)" }
    : { border: "var(--color-danger)", background: "var(--color-danger-soft)", text: "var(--color-danger)" };

  return (
    <div
      role={result.ok ? "status" : "alert"}
      style={{
        marginTop: 12,
        padding: "12px 14px",
        fontSize: 13,
        border: `1px solid ${palette.border}`,
        borderRadius: "var(--radius)",
        backgroundColor: palette.background,
      }}
    >
      <p style={{ margin: 0, fontWeight: 600, color: palette.text }}>
        {result.ok ? "连接测试成功" : "连接测试失败"}
      </p>
      <p style={{ margin: "4px 0 0", color: "var(--color-text)" }}>{result.message}</p>
      {typeof result.latencyMs === "number" ? (
        <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--color-muted)" }}>
          耗时 {result.latencyMs} ms
        </p>
      ) : null}
      {result.steps.length > 0 ? (
        <ul
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 6,
            margin: "10px 0 0",
            padding: 0,
            listStyle: "none",
          }}
        >
          {result.steps.map((step, index) => (
            <li
              key={`${step.name}-${index}`}
              style={{ display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 8 }}
            >
              <StatusBadge status={step.status} />
              <span style={{ fontWeight: 500 }}>{step.name}</span>
              {step.message ? (
                <span style={{ color: "var(--color-muted)" }}>{step.message}</span>
              ) : null}
              {typeof step.durationMs === "number" ? (
                <span style={{ fontSize: 12, color: "var(--color-muted)" }}>
                  {step.durationMs} ms
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {result.suggestion ? (
        <p style={{ margin: "10px 0 0", color: "var(--color-text)" }}>
          建议：{result.suggestion}
        </p>
      ) : null}
    </div>
  );
}
