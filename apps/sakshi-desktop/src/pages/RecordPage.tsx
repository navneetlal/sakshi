import { useCallback, useEffect, useMemo, useState } from "react";

import {
  api,
  errorText,
  type Permissions,
  type SetupCheck,
  type Settings,
  type Source,
  type SourceId,
} from "../api";
import { Badge, Dialog, ErrorNotice, JobSteps, Notice, Toggle } from "../components";
import { clock } from "../format";
import { tidy } from "../paths";
import { isActive, useApp, useJobs } from "../state";

const key = (id: SourceId) => `${id.kind}:${id.id}`;

export function RecordPage({ openMeeting }: { openMeeting: (id: string) => void }) {
  const { recorder } = useApp();
  return (
    <div className="page">
      <header className="page-header">
        <h1>Record</h1>
        <p className="muted">
          Records one window or display with its audio and your microphone. Transcription runs on
          this Mac after the call.
        </p>
      </header>
      {recorder.state === "recording" || recorder.state === "stopping" ? (
        <Recording />
      ) : (
        <Setup />
      )}
      <Processing openMeeting={openMeeting} />
    </div>
  );
}

function Recording() {
  const { recorder } = useApp();
  const [now, setNow] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);
  if (recorder.state === "stopping") {
    return (
      <section className="card recording stopping">
        <div className="rec-title">
          <span className="spinner" /> Finishing “{recorder.title}”…
        </div>
        <p className="muted">Closing the files. Processing starts right after.</p>
      </section>
    );
  }
  if (recorder.state !== "recording") return null;
  return (
    <section className="card recording">
      <div className="rec-row">
        <span className="rec-dot" aria-hidden="true" />
        <span className="rec-clock">{clock(now - recorder.started_at_ms)}</span>
      </div>
      <div className="rec-title">{recorder.title}</div>
      <div className="muted">{recorder.source}</div>
      <div className="rec-tracks">
        <Badge tone={recorder.system_audio ? "ok" : "neutral"}>
          App audio {recorder.system_audio ? "on" : "off"}
        </Badge>
        <Badge tone={recorder.microphone ? "ok" : "neutral"}>
          Microphone {recorder.microphone ? "on" : "off"}
        </Badge>
      </div>
      <ErrorNotice error={error} />
      <button
        className="btn primary large"
        onClick={() => api.stopRecording().catch((e) => setError(errorText(e)))}
      >
        Stop recording
      </button>
    </section>
  );
}

function Setup() {
  const { recorder } = useApp();
  const [permissions, setPermissions] = useState<Permissions | null>(null);
  const [sources, setSources] = useState<Source[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [microphone, setMicrophone] = useState(true);
  const [systemAudio, setSystemAudio] = useState(true);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [setup, setSetup] = useState<SetupCheck | null>(null);
  const [askConsent, setAskConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadingSources, setLoadingSources] = useState(false);

  const refreshPermissions = useCallback(
    () => api.permissions().then(setPermissions).catch((e) => setError(errorText(e))),
    [],
  );
  const refreshSources = useCallback(() => {
    setLoadingSources(true);
    api
      .listSources()
      .then((list) => {
        setSources(list);
        setSelected((current) => {
          if (current && list.some((s) => key(s.id) === current)) return current;
          const meeting = list.find((s) => s.meeting);
          return meeting ? key(meeting.id) : null;
        });
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setLoadingSources(false));
  }, []);

  useEffect(() => {
    void refreshPermissions();
    api.getSettings().then(setSettings).catch((e) => setError(errorText(e)));
    api.setupCheck().then(setSetup).catch((e) => setError(errorText(e)));
    // Coming back from System Settings: check access again.
    const onFocus = () => void refreshPermissions();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshPermissions]);

  const screenGranted = permissions?.screen === "granted";
  useEffect(() => {
    if (screenGranted) refreshSources();
  }, [screenGranted, refreshSources]);

  const source = sources?.find((s) => key(s.id) === selected) ?? null;
  const micReady = !microphone || permissions?.microphone === "granted";
  const canStart =
    screenGranted && micReady && source !== null && (microphone || systemAudio) &&
    recorder.state === "idle";

  const start = async () => {
    if (!source) return;
    setAskConsent(false);
    setError(null);
    try {
      await api.startRecording({
        source: source.id,
        title: title.trim() || null,
        microphone,
        systemAudio,
      });
      setTitle("");
    } catch (e) {
      setError(errorText(e));
    }
  };
  const onStart = () => (settings?.consent_reminder ? setAskConsent(true) : void start());

  if (permissions && !permissions.supported) {
    return (
      <Notice tone="warn" title="Recording isn't available on this platform yet">
        Windows support arrives in M5 and Linux in M6. You can still process recordings with
        sakshi-cli and browse meetings here.
      </Notice>
    );
  }

  const setupProblems = setup
    ? [setup.models, setup.transcriber, setup.ffmpeg].filter((c) => !c.ok)
    : [];

  return (
    <>
      {recorder.state === "idle" && recorder.notice && (
        <Notice tone="warn">{recorder.notice}</Notice>
      )}
      {setupProblems.length > 0 && (
        <Notice tone="warn" title="Recordings will be saved but not transcribed yet">
          <ul className="plain">
            {setupProblems.map((p) => (
              <li key={p.detail}>{tidy(p.detail)}</li>
            ))}
          </ul>
        </Notice>
      )}
      {permissions && !screenGranted && (
        <ScreenPermission permissions={permissions} onChange={setPermissions} />
      )}
      <ErrorNotice error={error} />

      {screenGranted && (
        <section className="card">
          <div className="card-header">
            <h2>What to record</h2>
            <button className="btn small" onClick={refreshSources} disabled={loadingSources}>
              {loadingSources ? "Looking…" : "Refresh"}
            </button>
          </div>
          <SourceList sources={sources} selected={selected} onSelect={setSelected} />
        </section>
      )}

      {screenGranted && (
        <section className="card">
          <h2>Options</h2>
          <label className="field">
            <span>Title</span>
            <input
              value={title}
              placeholder={source?.title || "Meeting title"}
              onChange={(e) => setTitle(e.target.value)}
            />
          </label>
          <Toggle
            checked={systemAudio}
            onChange={setSystemAudio}
            label="Record the app's audio"
            hint="Everyone else on the call."
          />
          <Toggle
            checked={microphone}
            onChange={setMicrophone}
            label="Record my microphone"
            hint="Your side of the call."
          />
          {microphone && permissions && permissions.microphone !== "granted" && (
            <MicPermission permissions={permissions} onChange={setPermissions} />
          )}
          <div className="actions">
            <button className="btn primary large" disabled={!canStart} onClick={onStart}>
              {recorder.state === "starting" ? "Starting…" : "Start recording"}
            </button>
            {!source && sources && <span className="muted">Pick a window or display first.</span>}
          </div>
        </section>
      )}

      {askConsent && settings && (
        <ConsentDialog
          onCancel={() => setAskConsent(false)}
          onStart={async (dontAskAgain) => {
            if (dontAskAgain) {
              try {
                setSettings(await api.saveSettings(settings.language, false));
              } catch (e) {
                setError(errorText(e));
              }
            }
            await start();
          }}
        />
      )}
    </>
  );
}

function SourceList({
  sources,
  selected,
  onSelect,
}: {
  sources: Source[] | null;
  selected: string | null;
  onSelect: (key: string) => void;
}) {
  const groups = useMemo(() => {
    const list = sources ?? [];
    const windows = list.filter((s) => !s.meeting && s.id.kind === "window");
    return [
      { name: "Looks like a meeting", items: list.filter((s) => s.meeting) },
      { name: "Windows", items: windows.filter((s) => s.on_screen) },
      {
        name: "Minimized, hidden, or on another desktop",
        items: windows.filter((s) => !s.on_screen),
      },
      { name: "Displays", items: list.filter((s) => s.id.kind === "display") },
    ].filter((g) => g.items.length > 0);
  }, [sources]);

  if (sources === null) return <p className="muted">Looking for windows…</p>;
  if (sources.length === 0) return <p className="muted">No windows or displays to record.</p>;
  return (
    <div className="sources">
      {groups.map((group) => (
        <div key={group.name}>
          <h3>{group.name}</h3>
          <div className="source-list" role="radiogroup" aria-label={group.name}>
            {group.items.map((s) => (
              <button
                key={key(s.id)}
                role="radio"
                aria-checked={selected === key(s.id)}
                className={`source ${selected === key(s.id) ? "selected" : ""}`}
                onClick={() => onSelect(key(s.id))}
              >
                <span className="source-app">
                  {s.id.kind === "display" ? "Entire screen" : (s.meeting ?? s.app_name ?? "Window")}
                  {s.meeting && <Badge tone="accent">Meeting</Badge>}
                </span>
                <span className="source-title">{s.title || "Untitled window"}</span>
                <span className="muted small">
                  {s.width}×{s.height}
                </span>
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function ScreenPermission({
  permissions,
  onChange,
}: {
  permissions: Permissions;
  onChange: (p: Permissions) => void;
}) {
  const [asked, setAsked] = useState(false);
  return (
    <Notice
      tone="warn"
      title="Sakshi needs Screen Recording access"
      actions={
        <>
          {!asked && permissions.screen === "not_determined" && (
            <button
              className="btn primary"
              onClick={() =>
                void api.requestPermission("screen").then((p) => {
                  setAsked(true);
                  onChange(p);
                })
              }
            >
              Allow
            </button>
          )}
          <button className="btn" onClick={() => void api.openPrivacySettings("screen")}>
            Open System Settings
          </button>
        </>
      }
    >
      Turn Sakshi on under Privacy &amp; Security › Screen &amp; System Audio Recording, then quit
      and reopen Sakshi. macOS only applies this setting to apps that start after it changes.
    </Notice>
  );
}

function MicPermission({
  permissions,
  onChange,
}: {
  permissions: Permissions;
  onChange: (p: Permissions) => void;
}) {
  if (permissions.microphone === "not_determined") {
    return (
      <Notice
        tone="warn"
        title="Microphone access"
        actions={
          <button
            className="btn primary"
            onClick={() => void api.requestPermission("microphone").then(onChange)}
          >
            Allow
          </button>
        }
      >
        macOS asks once whether Sakshi may use the microphone.
      </Notice>
    );
  }
  return (
    <Notice
      tone="warn"
      title="Microphone access is off"
      actions={
        <button className="btn" onClick={() => void api.openPrivacySettings("microphone")}>
          Open System Settings
        </button>
      }
    >
      Turn Sakshi on under Privacy &amp; Security › Microphone, or record without your microphone.
    </Notice>
  );
}

function ConsentDialog({
  onCancel,
  onStart,
}: {
  onCancel: () => void;
  onStart: (dontAskAgain: boolean) => Promise<void>;
}) {
  const [dontAskAgain, setDontAskAgain] = useState(false);
  return (
    <Dialog
      title="Let everyone know"
      onClose={onCancel}
      actions={
        <>
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn primary" autoFocus onClick={() => void onStart(dontAskAgain)}>
            Start recording
          </button>
        </>
      }
    >
      <p>
        Tell the other participants that you're recording this call. In many places recording
        without their consent is against the law.
      </p>
      <label className="check">
        <input
          type="checkbox"
          checked={dontAskAgain}
          onChange={(e) => setDontAskAgain(e.target.checked)}
        />
        Don't remind me again
      </label>
    </Dialog>
  );
}

function Processing({ openMeeting }: { openMeeting: (id: string) => void }) {
  const jobs = useJobs();
  const shown = [...jobs.filter(isActive), ...jobs.filter((j) => !isActive(j)).slice(0, 3)];
  if (shown.length === 0) return null;
  return (
    <section className="card">
      <h2>Processing</h2>
      <div className="jobs">
        {shown.map((job) => (
          <div key={job.id} className="job-card">
            <div className="job-header">
              <button className="link" onClick={() => openMeeting(job.meeting_id)}>
                {job.title}
              </button>
              {job.state === "queued" && <Badge tone="accent">Queued</Badge>}
              {job.state === "running" && <Badge tone="accent">Processing</Badge>}
              {job.state === "done" && <Badge tone="ok">Done</Badge>}
              {job.state === "failed" && <Badge tone="danger">Failed</Badge>}
            </div>
            {job.state !== "done" && <JobSteps job={job} />}
          </div>
        ))}
      </div>
    </section>
  );
}
