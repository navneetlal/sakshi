import { useCallback, useEffect, useState } from "react";

import { api, errorText, type ClientSetup, type ClientStatus, type McpStatus } from "../api";
import { Badge, ErrorNotice, Notice, Snippet, type Tone } from "../components";

const statusBadge: Record<ClientStatus, [Tone, string]> = {
  configured: ["ok", "Connected"],
  different: ["warn", "Set up with another command"],
  not_configured: ["neutral", "Not connected"],
  not_found: ["neutral", "Not installed"],
  unreadable: ["danger", "Config file can't be read"],
};

export function ConnectPage() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(
    () =>
      api
        .mcpStatus()
        .then(setStatus)
        .catch((e) => setError(errorText(e))),
    [],
  );
  useEffect(() => {
    void reload();
  }, [reload]);

  return (
    <div className="page">
      <header className="page-header">
        <h1>Connect AI clients</h1>
        <p className="muted">
          Clients such as Claude read your meetings through Sakshi's MCP server: transcripts with
          speakers, key frames, and search. Sakshi never summarizes; your client writes summaries
          and saves them back here.
        </p>
      </header>
      <ErrorNotice error={error} />
      {status?.error && (
        <Notice tone="warn" title="Can't set up clients yet">
          {status.error}
        </Notice>
      )}
      {status?.server && (
        <p className="muted small">
          Server: <span className="mono">{status.server}</span>
        </p>
      )}
      <div className="clients">
        {status?.clients.map((c) => (
          <ClientCard key={c.client} setup={c} onChanged={() => void reload()} />
        ))}
      </div>
      {status?.server && (
        <section className="card">
          <h2>Other clients</h2>
          <p className="muted">
            Clients that can launch a command use the same command as above. For clients that only
            speak HTTP, run the server on this Mac with a token; it listens on 127.0.0.1 only and
            refuses browsers.
          </p>
          <Snippet
            text={`"${status.server}" --http --port 8790 --token-file ~/.sakshi-mcp-token`}
          />
        </section>
      )}
    </div>
  );
}

function ClientCard({ setup, onChanged }: { setup: ClientSetup; onChanged: () => void }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tone, label] = statusBadge[setup.status];
  const canInstall =
    setup.can_write && (setup.status === "not_configured" || setup.status === "different");

  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      const outcome = await api.mcpInstall(setup.client);
      if (outcome.kind === "already_configured") {
        setMessage(`${setup.name} is already connected.`);
      } else {
        const restart =
          setup.client === "claude-desktop" ? " Quit and reopen Claude Desktop to use it." : "";
        const backup = outcome.backup ? ` The previous file is saved as ${outcome.backup}.` : "";
        setMessage(`Added Sakshi to ${outcome.path}.${backup}${restart}`);
      }
      onChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card client">
      <div className="card-header">
        <h2>{setup.name}</h2>
        <Badge tone={tone}>{label}</Badge>
      </div>
      <p>{setup.instructions}</p>
      <Snippet text={setup.snippet} />
      {canInstall && (
        <div className="actions">
          <button className="btn primary" disabled={busy} onClick={() => void install()}>
            Add Sakshi to {setup.name}
          </button>
          <span className="muted small">Edits {setup.config_path} after backing it up.</span>
        </div>
      )}
      {message && <Notice tone="ok">{message}</Notice>}
      <ErrorNotice error={error} />
    </section>
  );
}
