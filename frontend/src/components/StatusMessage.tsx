import { CheckCircle2, CircleAlert, Info } from "lucide-react";
import type { ReactNode } from "react";

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
  tone = "info"
}: {
  children: ReactNode;
  className?: string;
  tone?: StatusTone;
}) {
  const Icon = toneIcons[tone];
  const classes = ["status-message", tone, className].filter(Boolean).join(" ");
  return (
    <p className={classes} role={tone === "error" ? "alert" : "status"}>
      <Icon aria-hidden="true" size={15} />
      <span>{children}</span>
    </p>
  );
}
