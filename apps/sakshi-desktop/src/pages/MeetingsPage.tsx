import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { api, errorText, fileUrl, type MeetingDetail, type MeetingItem } from "../api";
import {
  Dialog,
  ErrorNotice,
  JobSteps,
  MeetingStatusBadge,
  Notice,
  languageLabel,
  speakerClass,
} from "../components";
import { duration, timestamp, when } from "../format";
import { Mark } from "../Logo";
import { useApp } from "../state";

export function MeetingsPage({
  selectedId,
  onSelect,
}: {
  selectedId: string | null;
  onSelect: (id: string | null) => void;
}) {
  const { meetingsVersion, jobs } = useApp();
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<MeetingItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Reload when a job starts or finishes, not on every step.
  const jobStates = useMemo(
    () =>
      Object.values(jobs)
        .map((j) => `${j.id}:${j.state}`)
        .join(","),
    [jobs],
  );

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .listMeetings(query.trim() || null)
        .then((list) => {
          if (!cancelled) {
            setItems(list);
            setError(null);
          }
        })
        .catch((e) => !cancelled && setError(errorText(e)));
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, meetingsVersion, jobStates]);

  const importRecording = async () => {
    try {
      const id = await api.importRecording();
      if (id) onSelect(id);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="split">
      <aside className="list-pane">
        <button className="btn import" onClick={() => void importRecording()}>
          Import recording…
        </button>
        <input
          className="search"
          type="search"
          placeholder="Search titles and transcripts"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <ErrorNotice error={error} />
        {items && items.length === 0 && (
          <p className="muted empty">
            {query
              ? "Nothing matches."
              : "No meetings yet. Record one on the Record page, or import a recording."}
          </p>
        )}
        <ul className="meeting-list">
          {items?.map((m) => (
            <li key={m.id}>
              <button
                className={`meeting-row ${m.id === selectedId ? "selected" : ""}`}
                onClick={() => onSelect(m.id)}
              >
                <span className="meeting-title">{m.title}</span>
                <span className="meeting-meta">
                  {when(m.started_at)}
                  {m.duration_ms ? ` · ${duration(m.duration_ms)}` : ""}
                </span>
                <MeetingStatusBadge status={m.status} />
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <section className="detail-pane">
        {selectedId ? (
          <MeetingView key={selectedId} id={selectedId} />
        ) : (
          <div className="empty-detail muted">
            {/* Clear space below the mark: half its height. */}
            <div style={{ marginBottom: 24 }}>
              <Mark cut="stacked" height={48} />
            </div>
            Choose a meeting to see its transcript.
          </div>
        )}
      </section>
    </div>
  );
}

type Tab = "transcript" | "frames" | "summaries";

function MeetingView({ id }: { id: string }) {
  const { meetingsVersion, jobs } = useApp();
  const [meeting, setMeeting] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("transcript");
  const [processOpen, setProcessOpen] = useState(false);
  const [namesOpen, setNamesOpen] = useState(false);
  const video = useRef<HTMLVideoElement>(null);

  // The newest job for this meeting, live from events.
  const job = useMemo(
    () =>
      Object.values(jobs)
        .filter((j) => j.meeting_id === id)
        .sort((a, b) => b.id - a.id)[0] ?? null,
    [jobs, id],
  );
  const jobState = job?.state;

  useEffect(() => {
    let cancelled = false;
    api
      .getMeeting(id)
      .then((m) => !cancelled && setMeeting(m))
      .catch((e) => !cancelled && setError(errorText(e)));
    return () => {
      cancelled = true;
    };
  }, [id, meetingsVersion, jobState]);

  if (error) return <ErrorNotice error={error} />;
  if (!meeting) return <p className="muted">Loading…</p>;

  const seek = (ms: number) => {
    const v = video.current;
    if (!v) return;
    v.currentTime = ms / 1000;
    void v.play();
  };
  const active = job && (job.state === "queued" || job.state === "running");
  const processed = meeting.segment_count > 0 || meeting.status === "processed";
  // The name the user gave a label, else the label; colours follow labels.
  const nameOf = (label: string) => meeting.speaker_names[label] ?? label;
  const labelHint = (label: string) => (label in meeting.speaker_names ? label : undefined);

  return (
    <article className="meeting">
      <header className="meeting-header">
        <div>
          <h1>{meeting.title}</h1>
          <p className="muted">
            {when(meeting.started_at)}
            {meeting.duration_ms ? ` · ${duration(meeting.duration_ms)}` : ""}
            {meeting.languages.length > 0 && ` · ${meeting.languages.map(languageLabel).join(", ")}`}
            {meeting.imported_from && ` · imported from ${meeting.imported_from}`}
          </p>
          {meeting.speakers.length > 0 && (
            <div className="speakers">
              {meeting.speakers.map((s) => (
                <span key={s} className={speakerClass(s)} title={labelHint(s)}>
                  {nameOf(s)}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="header-actions">
          <MeetingStatusBadge status={meeting.status} />
          {meeting.speakers.length > 0 && (
            <button className="btn" onClick={() => setNamesOpen(true)}>
              Name speakers…
            </button>
          )}
          <button className="btn" onClick={() => void api.revealMeeting(meeting.id)}>
            Show in Finder
          </button>
          <button
            className="btn"
            disabled={!!active || meeting.status === "recording"}
            onClick={() => setProcessOpen(true)}
          >
            {processed ? "Process again…" : "Process…"}
          </button>
        </div>
      </header>

      {job && (active || job.state === "failed") && <JobSteps job={job} />}
      {!processed && !active && meeting.status !== "recording" && (
        <Notice tone="warn" title="Not processed yet">
          This recording hasn't been transcribed. Choose Process to run it now.
        </Notice>
      )}

      {meeting.recording && (
        <video
          ref={video}
          className="player"
          // The time fragment makes WebKit show the first frame, not black.
          src={`${fileUrl(meeting.recording)}#t=0.1`}
          controls
          preload="metadata"
        />
      )}

      {processed && (
        <>
          <nav className="tabs" role="tablist">
            <TabButton tab="transcript" current={tab} onClick={setTab}>
              Transcript <span className="count">{meeting.segments.length}</span>
            </TabButton>
            <TabButton tab="frames" current={tab} onClick={setTab}>
              Key frames <span className="count">{meeting.frames.length}</span>
            </TabButton>
            <TabButton tab="summaries" current={tab} onClick={setTab}>
              Summaries <span className="count">{meeting.summaries.length}</span>
            </TabButton>
          </nav>
          {tab === "transcript" && (
            <ol className="transcript">
              {meeting.segments.map((s) => (
                <li key={s.id}>
                  <button
                    className="time"
                    onClick={() => seek(s.start_ms)}
                    disabled={!meeting.recording}
                    title="Play from here"
                  >
                    {timestamp(s.start_ms)}
                  </button>
                  <span className={speakerClass(s.speaker)} title={labelHint(s.speaker)}>
                    {nameOf(s.speaker)}
                  </span>
                  <p>{s.text}</p>
                </li>
              ))}
            </ol>
          )}
          {tab === "frames" &&
            (meeting.frames.length === 0 ? (
              <p className="muted">No key frames: this meeting has no video.</p>
            ) : (
              <div className="frames">
                {meeting.frames.map((f) => (
                  <figure key={f.at_ms}>
                    <button onClick={() => seek(f.at_ms)} title="Play from here">
                      <img
                        src={fileUrl(f.path)}
                        loading="lazy"
                        width={f.width}
                        height={f.height}
                        alt={`Screen at ${timestamp(f.at_ms)}`}
                      />
                    </button>
                    <figcaption>{timestamp(f.at_ms)}</figcaption>
                  </figure>
                ))}
              </div>
            ))}
          {tab === "summaries" &&
            (meeting.summaries.length === 0 ? (
              <p className="muted">
                No summaries yet. Sakshi never writes them: ask your AI client to summarize this
                meeting, and it saves the summary here through the save_summary tool.
              </p>
            ) : (
              <div className="summaries">
                {meeting.summaries.map((s) => (
                  <section key={s.id} className="card">
                    <h3>{s.title ?? "Summary"}</h3>
                    <p className="muted small">
                      {when(s.created_at)}
                      {s.author_model && ` · written by ${s.author_model}`}
                    </p>
                    <div className="markdown">{s.markdown}</div>
                  </section>
                ))}
              </div>
            ))}
        </>
      )}

      {processOpen && (
        <ProcessDialog
          meeting={meeting}
          processed={processed}
          onClose={() => setProcessOpen(false)}
          onError={setError}
        />
      )}
      {namesOpen && (
        <SpeakerNamesDialog
          meeting={meeting}
          onClose={() => setNamesOpen(false)}
          onError={setError}
        />
      )}
    </article>
  );
}

function SpeakerNamesDialog({
  meeting,
  onClose,
  onError,
}: {
  meeting: MeetingDetail;
  onClose: () => void;
  onError: (error: string) => void;
}) {
  const [names, setNames] = useState<Record<string, string>>(() =>
    Object.fromEntries(meeting.speakers.map((s) => [s, meeting.speaker_names[s] ?? ""])),
  );

  const save = async () => {
    try {
      await api.setSpeakerNames(meeting.id, names);
    } catch (e) {
      onError(errorText(e));
    }
    onClose();
  };

  return (
    <Dialog
      title="Name speakers"
      onClose={onClose}
      actions={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void save()}>
            Save
          </button>
        </>
      }
    >
      <p className="muted small">
        Names replace the labels in the transcript, here and for your AI client. Leave a name blank
        to show the label. If one person shows up as two speakers, give both the same name.
      </p>
      {meeting.speakers.map((label) => (
        <label key={label} className="field">
          <span className={speakerClass(label)}>{label}</span>
          <input
            value={names[label] ?? ""}
            maxLength={80}
            placeholder={label}
            onChange={(e) => setNames({ ...names, [label]: e.target.value })}
          />
        </label>
      ))}
    </Dialog>
  );
}

function TabButton({
  tab,
  current,
  onClick,
  children,
}: {
  tab: Tab;
  current: Tab;
  onClick: (tab: Tab) => void;
  children: ReactNode;
}) {
  return (
    <button
      role="tab"
      aria-selected={tab === current}
      className={tab === current ? "active" : ""}
      onClick={() => onClick(tab)}
    >
      {children}
    </button>
  );
}

function ProcessDialog({
  meeting,
  processed,
  onClose,
  onError,
}: {
  meeting: MeetingDetail;
  processed: boolean;
  onClose: () => void;
  onError: (error: string) => void;
}) {
  const [languages, setLanguages] = useState<string[] | null>(null);
  const [language, setLanguage] = useState(meeting.language ?? "");
  const [force, setForce] = useState(false);
  useEffect(() => {
    api
      .getSettings()
      .then((s) => setLanguages(s.supported_languages))
      .catch(() => setLanguages(null));
  }, []);

  const run = async () => {
    try {
      // An unchanged language isn't pinned again; "" means "as before".
      const pinned = language && language !== meeting.language ? language : null;
      await api.processMeeting(meeting.id, pinned, force);
      onClose();
    } catch (e) {
      onError(errorText(e));
      onClose();
    }
  };

  return (
    <Dialog
      title={processed ? "Process again" : "Process"}
      onClose={onClose}
      actions={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void run()}>
            Process
          </button>
        </>
      }
    >
      <label className="field">
        <span>Language</span>
        <select value={language} onChange={(e) => setLanguage(e.target.value)}>
          <option value="">
            {meeting.language ? languageLabel(meeting.language) : "As in Settings"}
          </option>
          <option value="auto">Detect automatically</option>
          {(languages ?? []).map((code) => (
            <option key={code} value={code}>
              {languageLabel(code)}
            </option>
          ))}
        </select>
      </label>
      <p className="muted small">
        Choosing a language pins it for this meeting. Stages that already finished with the same
        settings are reused.
      </p>
      {processed && (
        <label className="check">
          <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} />
          Run every stage again
        </label>
      )}
    </Dialog>
  );
}
