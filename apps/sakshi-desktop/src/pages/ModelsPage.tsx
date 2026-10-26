import { useCallback, useEffect, useState, type ReactNode } from "react";

import { api, errorText, type InspectedModel, type Model } from "../api";
import { Badge, Dialog, ErrorNotice } from "../components";
import { size } from "../format";
import { useApp } from "../state";

export function ModelsPage() {
  const { downloads } = useApp();
  const [models, setModels] = useState<Model[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<Model | null>(null);

  const reload = useCallback(
    () =>
      api
        .listModels()
        .then(setModels)
        .catch((e) => setError(errorText(e))),
    [],
  );
  // A finished download changes what's installed.
  const finished = Object.values(downloads)
    .filter((d) => d.state !== "running")
    .map((d) => `${d.model_id}:${d.state}`)
    .join(",");
  useEffect(() => {
    void reload();
  }, [reload, finished]);

  const download = async (model: Model) => {
    setConfirm(null);
    setError(null);
    try {
      await api.downloadModel(model.id, true);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="page">
      <header className="page-header">
        <h1>Models</h1>
        <p className="muted">
          Transcription and speaker separation run on this Mac with NVIDIA's Nemotron models.
          Downloads come from Hugging Face, only when you ask.
        </p>
      </header>
      <ErrorNotice error={error} />
      <section className="card">
        <table className="models">
          <thead>
            <tr>
              <th>Model</th>
              <th>Role</th>
              <th>Size</th>
              <th>License</th>
              <th aria-label="Status" />
            </tr>
          </thead>
          <tbody>
            {models?.map((m) => {
              const d = downloads[m.id] ?? m.download;
              return (
                <tr key={m.id}>
                  <td>
                    <button className="link strong" onClick={() => void api.openLink(m.repo_url)}>
                      {m.name}
                    </button>
                    <div className="muted small mono">{m.id}</div>
                    <div className="badges">
                      {m.default && <Badge>Default</Badge>}
                      {m.custom && <Badge tone="accent">Added by you</Badge>}
                    </div>
                  </td>
                  <td>{m.role === "asr" ? "Speech recognition" : "Speaker separation"}</td>
                  <td>{size(m.size)}</td>
                  <td>
                    <button className="link" onClick={() => void api.openLink(m.license_url)}>
                      {m.license}
                    </button>
                  </td>
                  <td className="status-cell">
                    {m.installed ? (
                      <Badge tone="ok">Downloaded</Badge>
                    ) : d?.state === "running" ? (
                      <div className="download">
                        <div className="progress">
                          <div style={{ width: `${(d.done / Math.max(1, d.total)) * 100}%` }} />
                        </div>
                        <span className="muted small">
                          {size(d.done)} of {size(d.total)}
                        </span>
                      </div>
                    ) : (
                      <>
                        {d?.state === "failed" && <p className="error-text small">{d.error}</p>}
                        <button className="btn primary" onClick={() => setConfirm(m)}>
                          {d?.state === "failed" ? "Try again" : "Download"}
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
      <AddModel onAdded={() => void reload()} />
      {confirm && (
        <LicenseDialog
          model={confirm}
          onCancel={() => setConfirm(null)}
          onAccept={() => void download(confirm)}
        />
      )}
    </div>
  );
}

function LicenseDialog({
  model,
  onCancel,
  onAccept,
  extra,
}: {
  model: Model;
  onCancel: () => void;
  onAccept: () => void;
  extra?: ReactNode;
}) {
  return (
    <Dialog
      title={`Download ${model.name}?`}
      onClose={onCancel}
      actions={
        <>
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn primary" onClick={onAccept}>
            Accept license and download
          </button>
        </>
      }
    >
      <dl className="facts">
        <dt>Repository</dt>
        <dd>
          <button className="link" onClick={() => void api.openLink(model.repo_url)}>
            {model.repo}
          </button>
        </dd>
        <dt>File</dt>
        <dd className="mono">{model.filename}</dd>
        <dt>Size</dt>
        <dd>{size(model.size)}</dd>
        <dt>License</dt>
        <dd>
          <button className="link" onClick={() => void api.openLink(model.license_url)}>
            {model.license}
          </button>
        </dd>
        <dt>Pinned to</dt>
        <dd className="mono">{model.revision.slice(0, 12)}</dd>
      </dl>
      {extra}
      <p className="muted small">
        The file is checked against its SHA-256 before Sakshi uses it.
      </p>
    </Dialog>
  );
}

function AddModel({ onAdded }: { onAdded: () => void }) {
  const [repo, setRepo] = useState("");
  const [file, setFile] = useState("");
  const [revision, setRevision] = useState("");
  const [checking, setChecking] = useState(false);
  const [inspected, setInspected] = useState<InspectedModel | null>(null);
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setChecking(true);
    setError(null);
    try {
      setInspected(await api.inspectModel(repo, file, revision.trim() || null));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setChecking(false);
    }
  };
  const add = async (model: InspectedModel) => {
    setInspected(null);
    try {
      await api.addModel(model.id, true);
      setRepo("");
      setFile("");
      setRevision("");
      onAdded();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <section className="card">
      <h2>Add a model</h2>
      <p className="muted">
        Any GGUF file on Hugging Face that the nemo-speech runtime can run: a Nemotron ASR model or
        a Sortformer diarization model. Sakshi reads the file's header before downloading
        anything. Converting NeMo checkpoints to GGUF is planned.
      </p>
      <div className="form-row">
        <label className="field">
          <span>Repository</span>
          <input
            value={repo}
            placeholder="owner/name"
            onChange={(e) => setRepo(e.target.value)}
          />
        </label>
        <label className="field grow">
          <span>File</span>
          <input
            value={file}
            placeholder="model-q8_0.gguf"
            onChange={(e) => setFile(e.target.value)}
          />
        </label>
        <label className="field">
          <span>Revision</span>
          <input
            value={revision}
            placeholder="main"
            onChange={(e) => setRevision(e.target.value)}
          />
        </label>
      </div>
      <ErrorNotice error={error} />
      <div className="actions">
        <button
          className="btn"
          disabled={checking || !repo.trim() || !file.trim()}
          onClick={() => void check()}
        >
          {checking ? "Checking…" : "Check model"}
        </button>
      </div>
      {inspected && (
        <LicenseDialog
          model={inspected}
          onCancel={() => setInspected(null)}
          onAccept={() => void add(inspected)}
          extra={
            <p>
              A {inspected.role === "asr" ? "speech recognition" : "speaker separation"} model
              (GGUF architecture <span className="mono">{inspected.architecture}</span>).
            </p>
          }
        />
      )}
    </section>
  );
}
