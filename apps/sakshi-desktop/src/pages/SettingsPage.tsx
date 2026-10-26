import { useEffect, useState } from "react";

import { api, errorText, type Check, type SetupCheck, type Settings } from "../api";
import { ErrorNotice, Toggle, languageLabel } from "../components";
import {
  PALETTES,
  applyTheme,
  loadTheme,
  resolveScheme,
  saveTheme,
  type Mode,
  type ThemeChoice,
} from "../theme";

export function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [setup, setSetup] = useState<SetupCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [customLanguage, setCustomLanguage] = useState("");

  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        setSettings(s);
        setCustomLanguage(s.language === "auto" ? "" : s.language);
      })
      .catch((e) => setError(errorText(e)));
    api.setupCheck().then(setSetup).catch((e) => setError(errorText(e)));
  }, []);

  useEffect(() => {
    if (!saved) return;
    const timer = window.setTimeout(() => setSaved(false), 1500);
    return () => window.clearTimeout(timer);
  }, [saved]);

  const save = async (language: string, consentReminder: boolean) => {
    setError(null);
    try {
      setSettings(await api.saveSettings(language, consentReminder));
      setSaved(true);
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (!settings) {
    return (
      <div className="page">
        <ErrorNotice error={error} />
      </div>
    );
  }
  const codes = settings.supported_languages;

  return (
    <div className="page">
      <header className="page-header">
        <h1>Settings</h1>
        {saved && <span className="saved">Saved</span>}
      </header>
      <ErrorNotice error={error} />

      <section className="card">
        <h2>Transcription language</h2>
        {codes ? (
          <label className="field">
            <span>Language</span>
            <select
              value={settings.language}
              onChange={(e) => void save(e.target.value, settings.consent_reminder)}
            >
              <option value="auto">Detect automatically</option>
              {codes.map((code) => (
                <option key={code} value={code}>
                  {languageLabel(code)}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <form
            className="form-row"
            onSubmit={(e) => {
              e.preventDefault();
              void save(customLanguage.trim() || "auto", settings.consent_reminder);
            }}
          >
            <label className="field grow">
              <span>Language code</span>
              <input
                value={customLanguage}
                placeholder="auto, or a code such as hi or en-US"
                onChange={(e) => setCustomLanguage(e.target.value)}
              />
            </label>
            <button className="btn" type="submit">
              Save
            </button>
          </form>
        )}
        <p className="muted small">
          Applies to every meeting unless one is pinned to its own language. Detection can miss
          the start of a sentence right after a speaker switches language; when you know the
          language, choose it here.
          {!codes && " Download the speech recognition model to pick from its languages."}
        </p>
      </section>

      <Appearance />

      <section className="card">
        <h2>Recording</h2>
        <Toggle
          checked={settings.consent_reminder}
          onChange={(on) => void save(settings.language, on)}
          label="Remind me to tell participants"
          hint="Shown each time before recording starts."
        />
      </section>

      <section className="card">
        <h2>Storage</h2>
        <p>
          Meetings, models, and the index live in <span className="mono">{settings.data_dir}</span>
          . Each meeting is a folder you can open, copy, or delete.
        </p>
        <button className="btn" onClick={() => void api.revealDataDir()}>
          Open in Finder
        </button>
      </section>

      {setup && (
        <section className="card">
          <h2>Processing tools</h2>
          <dl className="checks">
            <CheckRow name="FFmpeg" check={setup.ffmpeg} />
            <CheckRow name="Speech engine (nemo-speech)" check={setup.transcriber} />
            <CheckRow name="Models" check={setup.models} />
          </dl>
        </section>
      )}

      <section className="card">
        <h2>Privacy</h2>
        <p className="muted">
          Sakshi works offline. Recordings, transcripts, and summaries never leave this Mac unless
          you share them. The only network requests are model downloads you start. There's no
          telemetry.
        </p>
      </section>
    </div>
  );
}

const modes: { id: Mode; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

/** Palette and light/dark choice, remembered on this Mac. */
function Appearance() {
  const [choice, setChoice] = useState<ThemeChoice>(loadTheme);
  const change = (next: ThemeChoice) => {
    setChoice(next);
    saveTheme(next);
    applyTheme(next);
  };
  const scheme = resolveScheme(choice.mode);

  return (
    <section className="card">
      <h2>Appearance</h2>
      <div className="palettes" role="radiogroup" aria-label="Colour palette">
        {PALETTES.map((p) => (
          <button
            key={p.id}
            role="radio"
            aria-checked={choice.palette === p.id}
            className={`palette ${choice.palette === p.id ? "selected" : ""}`}
            onClick={() => change({ ...choice, palette: p.id })}
          >
            {/* Each preview carries its own palette and scheme. */}
            <span className="palette-preview" data-palette={p.id} data-scheme={scheme}>
              <span className="pv-sidebar" />
              <span className="pv-main">
                <span className="pv-line" />
                <span className="pv-line short" />
                <span className="pv-button" />
              </span>
            </span>
            <span className="palette-name">{p.name}</span>
            <span className="muted small">{p.description}</span>
          </button>
        ))}
      </div>
      <div className="segmented" role="radiogroup" aria-label="Light or dark">
        {modes.map((m) => (
          <button
            key={m.id}
            role="radio"
            aria-checked={choice.mode === m.id}
            className={choice.mode === m.id ? "active" : ""}
            onClick={() => change({ ...choice, mode: m.id })}
          >
            {m.label}
          </button>
        ))}
      </div>
    </section>
  );
}

function CheckRow({ name, check }: { name: string; check: Check }) {
  return (
    <>
      <dt>
        <span className={check.ok ? "ok-mark" : "fail-mark"} aria-hidden="true">
          {check.ok ? "✓" : "✕"}
        </span>
        {name}
      </dt>
      <dd className={check.ok ? "muted mono small" : "error-text"}>{check.detail}</dd>
    </>
  );
}
