export function ProgressScreen({
  detail,
  indeterminate,
  progress,
  title
}: {
  detail?: string;
  indeterminate: boolean;
  progress: number | null;
  title: string;
}) {
  return (
    <section className="screen center-screen">
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
