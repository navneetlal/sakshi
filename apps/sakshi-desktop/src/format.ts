/** 01:02:05 */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total / 60) % 60;
  const s = total % 60;
  return [h, m, s].map((n) => String(n).padStart(2, "0")).join(":");
}

/** 2:05 or 1:02:05, for timestamps inside a meeting. */
export function timestamp(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total / 60) % 60;
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** 42 min, 1 h 5 min */
export function duration(ms: number): string {
  const minutes = Math.round(ms / 60000);
  if (minutes < 1) return `${Math.max(1, Math.round(ms / 1000))} s`;
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

export function size(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

const dateFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

export function when(rfc3339: string | null): string {
  if (!rfc3339) return "Unknown time";
  const date = new Date(rfc3339);
  return Number.isNaN(date.getTime()) ? rfc3339 : dateFormat.format(date);
}

const stepNames: Record<string, string> = {
  import: "Copy the recording",
  finalize: "Finish recording",
  "prepare-audio": "Prepare audio",
  "transcribe-system": "Transcribe the call",
  "transcribe-mic": "Transcribe your mic",
  segments: "Label speakers",
  "remove-bleed": "Remove echo",
  keyframes: "Key frames",
  save: "Save transcript",
  index: "Add to library",
};

export function stepName(name: string): string {
  return stepNames[name] ?? name;
}
