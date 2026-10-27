// Typed access to the Rust backend. Shapes mirror the serde output of the
// structs in src-tauri/src; keep them in step when either side changes.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type PermissionState = "granted" | "denied" | "not_determined";
export type PermissionKind = "screen" | "microphone";

export interface Permissions {
  supported: boolean;
  screen: PermissionState;
  microphone: PermissionState;
}

export interface SourceId {
  kind: "window" | "display";
  id: number;
}

export interface Source {
  id: SourceId;
  title: string;
  app_name: string | null;
  bundle_id: string | null;
  width: number;
  height: number;
  /** The meeting app this looks like ("Zoom", "Microsoft Teams", "Google Meet"). */
  meeting: string | null;
  /** False when minimized, hidden, or on another desktop. */
  on_screen: boolean;
}

export type RecorderStatus =
  | { state: "idle"; notice: string | null }
  | { state: "starting" }
  | {
      state: "recording";
      meeting_id: string;
      title: string;
      source: string;
      started_at_ms: number;
      microphone: boolean;
      system_audio: boolean;
      /** The app being recorded, for messages ("Zoom"). */
      app: string;
      /** Set while the window's video is paused because it's out of view. */
      video_paused: "covered" | "hidden" | null;
    }
  | { state: "stopping"; meeting_id: string; title: string };

export interface StartRequest {
  source: SourceId;
  title: string | null;
  microphone: boolean;
  systemAudio: boolean;
}

export type StepState = "pending" | "running" | "done" | "reused" | "failed";

export interface Step {
  name: string;
  state: StepState;
  note: string | null;
}

export type Job = {
  id: number;
  meeting_id: string;
  title: string;
  steps: Step[];
} & (
  | { state: "queued" }
  | { state: "running" }
  | { state: "done" }
  | { state: "failed"; error: string }
);

export type MeetingStatus =
  | "recording"
  | "queued"
  | "processing"
  | "failed"
  | "unprocessed"
  | "processed";

export interface MeetingItem {
  id: string;
  title: string;
  started_at: string | null;
  duration_ms: number | null;
  speakers: string[];
  segment_count: number;
  frame_count: number;
  status: MeetingStatus;
  job: Job | null;
}

export interface Segment {
  id: number;
  start_ms: number;
  end_ms: number;
  track: "mic" | "system";
  speaker: string;
  text: string;
}

export interface Frame {
  at_ms: number;
  path: string;
  width: number;
  height: number;
}

export interface Summary {
  id: string;
  meeting_id: string;
  title: string | null;
  author_model: string | null;
  created_at: string;
  markdown: string;
}

export interface MeetingDetail extends MeetingItem {
  folder: string;
  recording: string | null;
  /** Pinned for this meeting: "auto" or a code. */
  language: string | null;
  languages: string[];
  segments: Segment[];
  frames: Frame[];
  summaries: Summary[];
}

export type Download = {
  model_id: string;
  done: number;
  total: number;
} & ({ state: "running" } | { state: "done" } | { state: "failed"; error: string });

export interface Model {
  id: string;
  name: string;
  role: "asr" | "diarization";
  repo: string;
  revision: string;
  filename: string;
  size: number;
  license: string;
  license_url: string;
  default: boolean;
  repo_url: string;
  installed: boolean;
  custom: boolean;
  download: Download | null;
}

export interface InspectedModel extends Model {
  architecture: string;
}

export interface Settings {
  language: string;
  consent_reminder: boolean;
  data_dir: string;
  supported_languages: string[] | null;
}

export interface Check {
  ok: boolean;
  detail: string;
}

export interface SetupCheck {
  ffmpeg: Check;
  transcriber: Check;
  models: Check;
}

export type ClientId = "claude-desktop" | "claude-code" | "codex" | "cursor";
export type ClientStatus =
  | "not_found"
  | "not_configured"
  | "different"
  | "configured"
  | "unreadable";

export interface ClientSetup {
  client: ClientId;
  name: string;
  config_path: string;
  status: ClientStatus;
  can_write: boolean;
  instructions: string;
  snippet: string;
}

export interface McpStatus {
  server: string | null;
  error: string | null;
  clients: ClientSetup[];
}

export type InstallOutcome =
  | { kind: "already_configured" }
  | { kind: "written"; path: string; backup: string | null };

export type AppEvent =
  | { kind: "recorder"; status: RecorderStatus }
  | { kind: "job"; job: Job }
  | { kind: "download"; download: Download }
  | { kind: "meetings_changed" };

export const api = {
  permissions: () => invoke<Permissions>("permissions"),
  requestPermission: (kind: PermissionKind) =>
    invoke<Permissions>("request_permission", { kind }),
  openPrivacySettings: (kind: PermissionKind) =>
    invoke<void>("open_privacy_settings", { kind }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  listSources: () => invoke<Source[]>("list_sources"),
  recorderStatus: () => invoke<RecorderStatus>("recorder_status"),
  startRecording: (request: StartRequest) =>
    invoke<RecorderStatus>("start_recording", { request }),
  stopRecording: () => invoke<void>("stop_recording"),
  showRecordedWindow: () => invoke<void>("show_recorded_window"),
  listMeetings: (query: string | null) =>
    invoke<MeetingItem[]>("list_meetings", { query }),
  getMeeting: (id: string) => invoke<MeetingDetail>("get_meeting", { id }),
  processMeeting: (id: string, language: string | null, force: boolean) =>
    invoke<Job>("process_meeting", { id, language, force }),
  setMeetingLanguage: (id: string, language: string | null) =>
    invoke<void>("set_meeting_language", { id, language }),
  revealMeeting: (id: string) => invoke<void>("reveal_meeting", { id }),
  listJobs: () => invoke<Job[]>("list_jobs"),
  listModels: () => invoke<Model[]>("list_models"),
  downloadModel: (id: string, licenseAccepted: boolean) =>
    invoke<void>("download_model", { id, licenseAccepted }),
  inspectModel: (repo: string, file: string, revision: string | null) =>
    invoke<InspectedModel>("inspect_model", { repo, file, revision }),
  addModel: (id: string, licenseAccepted: boolean) =>
    invoke<void>("add_model", { id, licenseAccepted }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (language: string, consentReminder: boolean) =>
    invoke<Settings>("save_settings", { language, consentReminder }),
  setupCheck: () => invoke<SetupCheck>("setup_check"),
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  mcpInstall: (client: ClientId) => invoke<InstallOutcome>("mcp_install", { client }),
};

export function onAppEvent(handler: (event: AppEvent) => void): Promise<UnlistenFn> {
  return listen<AppEvent>("sakshi", (e) => handler(e.payload));
}

/** A URL the webview can load for a file inside a meeting folder. */
export function fileUrl(path: string): string {
  return convertFileSrc(path);
}

/** Backend errors arrive as strings; anything else is a bug worth showing. */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
