import { CheckCircle2, CircleAlert, Copy, Info } from "lucide-react";
import { useState, type ReactNode } from "react";

export type StatusTone = "info" | "success" | "error";

export type Feedback = {
  message: string;
  tone: StatusTone;
};

const toneIcons = {
  error: CircleAlert,
  info: Info,
  success: CheckCircle2
};

export function StatusMessage({
  children,
  className,
  detail,
  tone = "info"
}: {
  children: ReactNode;
  className?: string;
  detail?: string;
  tone?: StatusTone;
}) {
  const Icon = toneIcons[tone];
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const copyDetail = async () => {
    if (!detail) return;
    try {
      await navigator.clipboard.writeText(detail);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };
  const classes = ["status-message", tone, className].filter(Boolean).join(" ");
  return (
    <p className={classes} role={tone === "error" ? "alert" : "status"}>
      <Icon aria-hidden="true" size={15} />
      <span>
        {children}
        {detail ? (
          <span className="error-detail-actions">
            <button className="button secondary" onClick={() => void copyDetail()} type="button">
              <Copy aria-hidden="true" size={15} />复制错误详情
            </button>
            {copyState === "copied" ? <span role="status">已复制，可粘贴到反馈中</span> : null}
            {copyState === "failed" ? <span role="alert">复制失败，请重试</span> : null}
          </span>
        ) : null}
      </span>
    </p>
  );
}
