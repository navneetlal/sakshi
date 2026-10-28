//! Processing runs one meeting at a time on a worker thread, so a long
//! transcription never blocks recording the next call.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use sakshi_app::{ProcessOptions, Progress};
use sakshi_pipeline::{Stage, StageEvent};
use serde::Serialize;

use crate::events::{AppEvent, Notify};

/// Processes one meeting folder, reporting progress, and returns the
/// meeting's id in the index.
pub type Processor = dyn Fn(&Path, &ProcessOptions, &(dyn Fn(Progress) + Send + Sync)) -> anyhow::Result<String>
    + Send
    + Sync;

/// How many finished jobs to remember for the UI.
const KEEP_FINISHED: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobView {
    pub id: u64,
    /// The meeting folder's name, which is the meeting id for recordings.
    pub meeting_id: String,
    pub title: String,
    #[serde(flatten)]
    pub state: JobState,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub name: &'static str,
    pub state: StepState,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Running,
    Done,
    /// Finished in an earlier run.
    Reused,
    Failed,
}

const IMPORT: &str = "import";
const FINALIZE: &str = "finalize";
const INDEX: &str = "index";

impl JobView {
    fn new(id: u64, meeting_id: String, title: String) -> Self {
        let steps = Stage::ALL
            .iter()
            .map(|s| s.as_str())
            .chain([INDEX])
            .map(|name| Step {
                name,
                state: StepState::Pending,
                note: None,
            })
            .collect();
        Self {
            id,
            meeting_id,
            title,
            state: JobState::Queued,
            steps,
        }
    }

    fn step(&mut self, name: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.name == name)
    }

    fn apply(&mut self, progress: &Progress) {
        match progress {
            Progress::Importing => self.start_extra_step(IMPORT),
            Progress::Finalizing => self.start_extra_step(FINALIZE),
            Progress::Stage { event } => {
                self.finish(IMPORT);
                self.finish(FINALIZE);
                let (stage, state, note) = match event {
                    StageEvent::Started { stage } => (stage, StepState::Running, None),
                    StageEvent::Reused { stage } => (stage, StepState::Reused, None),
                    StageEvent::Finished { stage, note } => (stage, StepState::Done, note.clone()),
                    StageEvent::Failed { stage, error } => {
                        (stage, StepState::Failed, Some(error.clone()))
                    }
                };
                if let Some(step) = self.step(stage.as_str()) {
                    step.state = state;
                    step.note = note;
                }
            }
            Progress::Indexing => {
                if let Some(step) = self.step(INDEX) {
                    step.state = StepState::Running;
                }
            }
        }
    }

    /// Adds a step only some meetings need, first and running.
    fn start_extra_step(&mut self, name: &'static str) {
        self.steps.insert(
            0,
            Step {
                name,
                state: StepState::Running,
                note: None,
            },
        );
    }

    fn finish(&mut self, name: &str) {
        if let Some(step) = self.step(name).filter(|s| s.state == StepState::Running) {
            step.state = StepState::Done;
        }
    }

    fn is_finished(&self) -> bool {
        matches!(self.state, JobState::Done | JobState::Failed { .. })
    }
}

struct Request {
    id: u64,
    folder: PathBuf,
    options: ProcessOptions,
}

#[derive(Default)]
struct Board {
    next_id: u64,
    jobs: Vec<JobView>,
}

pub struct Jobs {
    board: Arc<Mutex<Board>>,
    queue: Mutex<Sender<Request>>,
}

impl Jobs {
    /// Starts the worker thread.
    pub fn start(processor: Arc<Processor>, notify: Arc<dyn Notify>) -> Self {
        let board = Arc::new(Mutex::new(Board::default()));
        let (tx, rx) = channel();
        let worker_board = Arc::clone(&board);
        std::thread::Builder::new()
            .name("sakshi-processing".into())
            .spawn(move || work(rx, &worker_board, processor.as_ref(), notify.as_ref()))
            .expect("starting the processing thread");
        Self {
            board,
            queue: Mutex::new(tx),
        }
    }

    /// Queues a meeting's folder. A meeting already queued or running isn't
    /// queued twice; its job is returned instead.
    pub fn enqueue(
        &self,
        meeting_id: String,
        folder: PathBuf,
        title: String,
        options: ProcessOptions,
        notify: &dyn Notify,
    ) -> JobView {
        let job = {
            let mut board = self.board.lock().expect("job board");
            if let Some(job) = board
                .jobs
                .iter()
                .find(|j| j.meeting_id == meeting_id && !j.is_finished())
            {
                return job.clone();
            }
            board.next_id += 1;
            let job = JobView::new(board.next_id, meeting_id, title);
            board.jobs.push(job.clone());
            let finished = board.jobs.iter().filter(|j| j.is_finished()).count();
            if finished > KEEP_FINISHED {
                let mut drop = finished - KEEP_FINISHED;
                board.jobs.retain(|j| {
                    let gone = drop > 0 && j.is_finished();
                    drop -= usize::from(gone);
                    !gone
                });
            }
            job
        };
        notify.notify(AppEvent::Job { job: job.clone() });
        let request = Request {
            id: job.id,
            folder,
            options,
        };
        if self.queue.lock().expect("job queue").send(request).is_err() {
            tracing::error!("the processing thread has stopped");
        }
        job
    }

    /// Recent and current jobs, oldest first.
    pub fn list(&self) -> Vec<JobView> {
        self.board.lock().expect("job board").jobs.clone()
    }

    /// The newest job for a meeting.
    pub fn for_meeting(&self, meeting_id: &str) -> Option<JobView> {
        let board = self.board.lock().expect("job board");
        board
            .jobs
            .iter()
            .rev()
            .find(|j| j.meeting_id == meeting_id)
            .cloned()
    }
}

fn work(rx: Receiver<Request>, board: &Mutex<Board>, processor: &Processor, notify: &dyn Notify) {
    let update = |id: u64, change: &dyn Fn(&mut JobView)| {
        let job = {
            let mut board = board.lock().expect("job board");
            let Some(job) = board.jobs.iter_mut().find(|j| j.id == id) else {
                return;
            };
            change(job);
            job.clone()
        };
        notify.notify(AppEvent::Job { job });
    };
    for request in rx {
        let id = request.id;
        update(id, &|job| job.state = JobState::Running);
        let progress = |p: Progress| update(id, &|job| job.apply(&p));
        match processor(&request.folder, &request.options, &progress) {
            Ok(_) => {
                update(id, &|job| {
                    job.finish(INDEX);
                    job.state = JobState::Done;
                });
                notify.notify(AppEvent::MeetingsChanged);
            }
            Err(e) => {
                let error = format!("{e:#}");
                tracing::warn!(folder = %request.folder.display(), "processing failed: {error}");
                update(id, &|job| {
                    for step in &mut job.steps {
                        if step.state == StepState::Running {
                            step.state = StepState::Failed;
                        }
                    }
                    job.state = JobState::Failed {
                        error: error.clone(),
                    };
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::testing::Recorded;
    use crate::test_util::wait_for;

    fn fake_processor() -> Arc<Processor> {
        Arc::new(|folder, _options, progress| {
            if folder.ends_with("broken") {
                progress(Progress::Stage {
                    event: StageEvent::Started {
                        stage: Stage::PrepareAudio,
                    },
                });
                anyhow::bail!("no audio in {}", folder.display());
            }
            progress(Progress::Finalizing);
            for stage in Stage::ALL {
                progress(Progress::Stage {
                    event: StageEvent::Started { stage },
                });
                progress(Progress::Stage {
                    event: StageEvent::Finished { stage, note: None },
                });
            }
            progress(Progress::Indexing);
            Ok("ok".into())
        })
    }

    #[test]
    fn runs_jobs_and_reports_every_step() {
        let notify = Arc::new(Recorded::default());
        let jobs = Jobs::start(fake_processor(), notify.clone());

        let job = jobs.enqueue(
            "one".into(),
            "/m/one".into(),
            "One".into(),
            ProcessOptions::default(),
            notify.as_ref(),
        );
        assert_eq!(job.state, JobState::Queued);
        wait_for("the job to finish", || {
            jobs.for_meeting("one").is_some_and(|j| j.is_finished())
        });

        let job = jobs.for_meeting("one").unwrap();
        assert_eq!(job.state, JobState::Done);
        assert_eq!(job.steps.first().unwrap().name, FINALIZE);
        assert!(
            job.steps.iter().all(|s| s.state == StepState::Done),
            "{:?}",
            job.steps
        );
        assert_eq!(job.steps.len(), Stage::ALL.len() + 2);
        let events = notify.take();
        assert!(events.contains(&AppEvent::MeetingsChanged));
    }

    #[test]
    fn a_failure_marks_the_running_step_and_the_job() {
        let notify = Arc::new(Recorded::default());
        let jobs = Jobs::start(fake_processor(), notify.clone());

        jobs.enqueue(
            "broken".into(),
            "/m/broken".into(),
            "Broken".into(),
            ProcessOptions::default(),
            notify.as_ref(),
        );
        wait_for("the job to fail", || {
            jobs.for_meeting("broken").is_some_and(|j| j.is_finished())
        });

        let job = jobs.for_meeting("broken").unwrap();
        assert_eq!(
            job.state,
            JobState::Failed {
                error: "no audio in /m/broken".into()
            }
        );
        assert_eq!(job.steps[0].name, "prepare-audio");
        assert_eq!(job.steps[0].state, StepState::Failed);
        assert!(!notify.take().contains(&AppEvent::MeetingsChanged));
    }

    #[test]
    fn a_meeting_is_queued_once_at_a_time() {
        let notify = Arc::new(Recorded::default());
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let processor_gate = Arc::clone(&gate);
        let processor: Arc<Processor> = Arc::new(move |_, _, _| {
            drop(processor_gate.lock().unwrap());
            Ok("ok".into())
        });
        let jobs = Jobs::start(processor, notify.clone());

        let first = jobs.enqueue(
            "a".into(),
            "/m/a".into(),
            "A".into(),
            ProcessOptions::default(),
            notify.as_ref(),
        );
        let again = jobs.enqueue(
            "a".into(),
            "/m/a".into(),
            "A".into(),
            ProcessOptions::default(),
            notify.as_ref(),
        );
        assert_eq!(first.id, again.id);
        drop(held);
        wait_for("the job to finish", || {
            jobs.for_meeting("a").is_some_and(|j| j.is_finished())
        });
        assert_eq!(jobs.list().len(), 1);
    }
}
