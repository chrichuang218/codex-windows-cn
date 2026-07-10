export function ProgressScreen({
  brandMark,
  compact = false,
  detail,
  indeterminate,
  progress,
  title
}: {
  brandMark?: string;
  compact?: boolean;
  detail?: string;
  indeterminate: boolean;
  progress: number | null;
  title: string;
}) {
  return (
    <section className={compact ? "screen center-screen boot-progress-screen" : "screen center-screen"}>
      {brandMark ? <div aria-hidden="true" className="assistant-mark">{brandMark}</div> : null}
      <h2>{title}</h2>
      <div
        aria-label={title}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={progress ?? undefined}
        className={indeterminate ? "progress-track progress-indeterminate" : "progress-track"}
        role="progressbar"
      >
        <div style={{ width: `${progress ?? 36}%` }} />
      </div>
      <span className="progress-meta">{progress === null ? "准备中" : `${progress}%`}</span>
      {detail ? <p className="muted">{detail}</p> : null}
    </section>
  );
}

export function ProgressInline({
  detail,
  progress,
  title
}: {
  detail?: string;
  progress: number | null;
  title: string;
}) {
  return (
    <div className="inline-progress">
      <strong>{title}</strong>
      {detail ? <span>{detail}</span> : null}
      <div
        aria-label={title}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={progress ?? undefined}
        className={progress === null ? "progress-track progress-indeterminate" : "progress-track"}
        role="progressbar"
      >
        <div style={{ width: `${progress ?? 36}%` }} />
      </div>
      <span className="progress-meta">{progress === null ? "准备中" : `${progress}%`}</span>
    </div>
  );
}
