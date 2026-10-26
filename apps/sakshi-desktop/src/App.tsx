import { useEffect, useState, type ReactNode } from "react";

import { clock } from "./format";
import { ConnectPage } from "./pages/ConnectPage";
import { MeetingsPage } from "./pages/MeetingsPage";
import { ModelsPage } from "./pages/ModelsPage";
import { RecordPage } from "./pages/RecordPage";
import { SettingsPage } from "./pages/SettingsPage";
import { isActive, useApp, useJobs } from "./state";

type Page = "record" | "meetings" | "models" | "connect" | "settings";

const pages: { id: Page; label: string; icon: ReactNode }[] = [
  { id: "record", label: "Record", icon: <circle cx="10" cy="10" r="5" /> },
  {
    id: "meetings",
    label: "Meetings",
    icon: <path d="M4 5h12M4 10h12M4 15h8" />,
  },
  {
    id: "models",
    label: "Models",
    icon: <path d="M10 3l6 3.5v7L10 17l-6-3.5v-7zM10 10l6-3.5M10 10v7M10 10L4 6.5" />,
  },
  {
    id: "connect",
    label: "Connect",
    icon: <path d="M8 12l4-4M7 9l-2 2a3 3 0 004 4l2-2M13 11l2-2a3 3 0 00-4-4L9 7" />,
  },
  {
    id: "settings",
    label: "Settings",
    icon: (
      <>
        <circle cx="10" cy="10" r="2.5" />
        <path d="M10 2.5v2.5M10 15v2.5M2.5 10H5M15 10h2.5M4.7 4.7l1.8 1.8M13.5 13.5l1.8 1.8M4.7 15.3l1.8-1.8M13.5 6.5l1.8-1.8" />
      </>
    ),
  },
];

/** `#/meetings/<id>`: where the window is, so a reload comes back to it. */
function readHash(): { page: Page; meetingId: string | null } {
  const [, page, id] = window.location.hash.split("/");
  const known = pages.find((p) => p.id === page);
  return { page: known?.id ?? "record", meetingId: (known && id) || null };
}

export function App() {
  const [{ page, meetingId }, setLocation] = useState(readHash);
  const setPage = (next: Page) => setLocation((l) => ({ ...l, page: next }));
  const setMeetingId = (id: string | null) => setLocation((l) => ({ ...l, meetingId: id }));

  useEffect(() => {
    const hash = page === "meetings" && meetingId ? `#/meetings/${meetingId}` : `#/${page}`;
    window.history.replaceState(null, "", hash);
  }, [page, meetingId]);

  const openMeeting = (id: string) => setLocation({ page: "meetings", meetingId: id });

  return (
    <div className="app">
      <nav className="sidebar" aria-label="Sakshi">
        <div className="brand">
          <img src="/icon.png" alt="" width="28" height="28" />
          <span>Sakshi</span>
        </div>
        <ul>
          {pages.map((p) => (
            <li key={p.id}>
              <button
                className={page === p.id ? "active" : ""}
                aria-current={page === p.id ? "page" : undefined}
                onClick={() => setPage(p.id)}
              >
                <svg viewBox="0 0 20 20" aria-hidden="true">
                  {p.icon}
                </svg>
                {p.label}
              </button>
            </li>
          ))}
        </ul>
        <SidebarStatus onRecord={() => setPage("record")} />
      </nav>
      <main className="content">
        {page === "record" && <RecordPage openMeeting={openMeeting} />}
        {page === "meetings" && <MeetingsPage selectedId={meetingId} onSelect={setMeetingId} />}
        {page === "models" && <ModelsPage />}
        {page === "connect" && <ConnectPage />}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}

function SidebarStatus({ onRecord }: { onRecord: () => void }) {
  const { recorder } = useApp();
  const active = useJobs().filter(isActive).length;
  const [now, setNow] = useState(() => Date.now());
  const recording = recorder.state === "recording";
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  return (
    <div className="sidebar-status">
      {recording && (
        <button className="live" onClick={onRecord}>
          <span className="rec-dot" aria-hidden="true" />
          Recording {clock(now - recorder.started_at_ms)}
        </button>
      )}
      {active > 0 && (
        <div className="processing">
          <span className="spinner" aria-hidden="true" />
          Processing {active === 1 ? "1 meeting" : `${active} meetings`}
        </div>
      )}
    </div>
  );
}
