type AssistantMarkProps = {
  className?: string;
  label?: string;
};

export function AssistantMark({ className, label }: AssistantMarkProps) {
  const classes = className ? `assistant-mark ${className}` : "assistant-mark";

  return (
    <div
      aria-hidden={label ? undefined : true}
      aria-label={label}
      className={classes}
      role={label ? "img" : undefined}
    >
      <img alt="" draggable={false} src="/chatgpt-mark.svg" />
    </div>
  );
}
