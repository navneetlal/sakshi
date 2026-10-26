import { useEffect, useState, type ReactNode } from "react";

import type { Job, MeetingStatus, StepState } from "./api";
import { stepName } from "./format";

export type Tone = "neutral" | "accent" | "ok" | "warn" | "danger" | "live";

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`badge ${tone}`}>{children}</span>;
}

const meetingStatus: Record<MeetingStatus, [Tone, string]> = {
  recording: ["live", "Recording"],
  queued: ["accent", "Queued"],
  processing: ["accent", "Processing"],
  failed: ["danger", "Failed"],
  unprocessed: ["warn", "Not processed"],
  processed: ["neutral", "Ready"],
};

export function MeetingStatusBadge({ status }: { status: MeetingStatus }) {
  const [tone, label] = meetingStatus[status];
  return status === "processed" ? null : <Badge tone={tone}>{label}</Badge>;
}

export function Notice({
  tone = "neutral",
  title,
  children,
  actions,
}: {
  tone?: Tone;
  title?: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className={`notice ${tone}`} role={tone === "danger" ? "alert" : undefined}>
      <div className="notice-body">
        {title && <strong>{title}</strong>}
        {children && <div>{children}</div>}
      </div>
      {actions && <div className="notice-actions">{actions}</div>}
    </div>
  );
}

export function ErrorNotice({ error }: { error: string | null }) {
  return error ? <Notice tone="danger">{error}</Notice> : null;
}

export function Dialog({
  title,
  children,
  actions,
  onClose,
}: {
  title: string;
  children: ReactNode;
  actions: ReactNode;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        <div className="dialog-body">{children}</div>
        <div className="dialog-actions">{actions}</div>
      </div>
    </div>
  );
}

const stepMark: Record<StepState, string> = {
  pending: "",
  running: "",
  done: "✓",
  reused: "✓",
  failed: "✕",
};

export function JobSteps({ job }: { job: Job }) {
  const finished = job.steps.filter((s) => s.state === "done" || s.state === "reused").length;
  const percent = Math.round((finished / Math.max(1, job.steps.length)) * 100);
  return (
    <div className="job">
      <div className="progress" aria-label={`${percent}% processed`}>
        <div style={{ width: `${percent}%` }} />
      </div>
      <ol className="steps">
        {job.steps.map((step) => (
          <li key={step.name} className={step.state} title={step.note ?? undefined}>
            <span className="step-mark" aria-hidden="true">
              {step.state === "running" ? <span className="spinner" /> : stepMark[step.state]}
            </span>
            {stepName(step.name)}
            {step.state === "reused" && <span className="muted"> (already done)</span>}
          </li>
        ))}
      </ol>
      {job.state === "failed" && <p className="error-text">{job.error}</p>}
    </div>
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <button
      className="btn small"
      onClick={() => void navigator.clipboard.writeText(text).then(() => setCopied(true))}
    >
      {copied ? "Copied" : label}
    </button>
  );
}

export function Snippet({ text }: { text: string }) {
  return (
    <div className="snippet">
      <pre>{text}</pre>
      <CopyButton text={text} />
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  hint?: ReactNode;
  disabled?: boolean;
}) {
  return (
    <label className={`toggle ${disabled ? "disabled" : ""}`}>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="switch" aria-hidden="true" />
      <span>
        {label}
        {hint && <span className="hint">{hint}</span>}
      </span>
    </label>
  );
}

const languageNames = new Intl.DisplayNames(undefined, { type: "language" });

/** "Hindi (hi)", or the code alone when the browser doesn't know it. */
export function languageLabel(code: string): string {
  if (code === "auto") return "Detect automatically";
  try {
    const name = languageNames.of(code);
    return name && name !== code ? `${name} (${code})` : code;
  } catch {
    return code;
  }
}

/** Speaker colors: "You" uses the accent, remote speakers cycle a palette. */
export function speakerClass(speaker: string): string {
  if (speaker === "You") return "speaker you";
  const n = Number.parseInt(speaker.replace(/\D/g, ""), 10);
  return `speaker s${Number.isNaN(n) ? 0 : (n - 1) % 6}`;
}
