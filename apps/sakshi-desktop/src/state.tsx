// App-wide live state: the recorder, processing jobs, and model downloads,
// kept current by the backend's events.

import {
  createContext,
  useContext,
  useEffect,
  useReducer,
  type ReactNode,
} from "react";

import { api, onAppEvent, type AppEvent, type Download, type Job, type RecorderStatus } from "./api";

interface State {
  recorder: RecorderStatus;
  jobs: Record<number, Job>;
  downloads: Record<string, Download>;
  /** Bumped whenever the meeting index changes. */
  meetingsVersion: number;
}

type Action = AppEvent | { kind: "loaded"; recorder: RecorderStatus; jobs: Job[] };

const initial: State = {
  recorder: { state: "idle", notice: null },
  jobs: {},
  downloads: {},
  meetingsVersion: 0,
};

function reduce(state: State, action: Action): State {
  switch (action.kind) {
    case "loaded": {
      const jobs: Record<number, Job> = { ...state.jobs };
      for (const job of action.jobs) jobs[job.id] = job;
      return { ...state, recorder: action.recorder, jobs };
    }
    case "recorder":
      return { ...state, recorder: action.status };
    case "job":
      return { ...state, jobs: { ...state.jobs, [action.job.id]: action.job } };
    case "download":
      return {
        ...state,
        downloads: { ...state.downloads, [action.download.model_id]: action.download },
      };
    case "meetings_changed":
      return { ...state, meetingsVersion: state.meetingsVersion + 1 };
  }
}

const Context = createContext<State>(initial);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reduce, initial);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    // Subscribe first so nothing that happens during the initial load is lost.
    void onAppEvent(dispatch).then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    });
    void Promise.all([api.recorderStatus(), api.listJobs()]).then(([recorder, jobs]) => {
      if (!cancelled) dispatch({ kind: "loaded", recorder, jobs });
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return <Context.Provider value={state}>{children}</Context.Provider>;
}

export function useApp(): State {
  return useContext(Context);
}

/** Jobs, newest first. */
export function useJobs(): Job[] {
  const { jobs } = useApp();
  return Object.values(jobs).sort((a, b) => b.id - a.id);
}

export function isActive(job: Job): boolean {
  return job.state === "queued" || job.state === "running";
}
