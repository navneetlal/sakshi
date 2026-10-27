//! Constant-frame-rate video from frames that arrive only when the screen
//! changes.
//!
//! Capture callbacks put the newest frame in a [`FrameSlot`]. A ticker thread
//! writes exactly one frame per tick to an ffmpeg encoder, starting at the
//! session start, so frame `n` is `n / fps` seconds into the session: the same
//! clock as the audio tracks. Output is fragmented MP4, readable up to the
//! last fragment if the process dies.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sakshi_media::Ffmpeg;

use crate::CaptureError;

/// One BGRA frame, tightly packed (`width * 4` bytes per row).
#[derive(Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Arc<Vec<u8>>,
}

/// The most recent frame from the capture callback.
#[derive(Default)]
pub struct FrameSlot {
    latest: Mutex<Option<VideoFrame>>,
    received: AtomicU64,
}

impl FrameSlot {
    pub fn put(&self, frame: VideoFrame) {
        *self.latest.lock().expect("frame slot lock") = Some(frame);
        self.received.fetch_add(1, Ordering::Relaxed);
    }

    fn latest(&self) -> Option<VideoFrame> {
        self.latest.lock().expect("frame slot lock").clone()
    }

    /// Frames put so far. Capture only sends one when the picture changes.
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }
}

/// An ffmpeg process encoding raw BGRA frames from stdin.
pub struct VideoEncoder {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: Option<JoinHandle<String>>,
    width: u32,
    height: u32,
    path: PathBuf,
}

impl VideoEncoder {
    pub fn spawn(
        ffmpeg: &Ffmpeg,
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        codec: &str,
    ) -> Result<Self, CaptureError> {
        let mut cmd = Command::new(ffmpeg.path());
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra"])
            .args([
                "-s",
                &format!("{width}x{height}"),
                "-framerate",
                &fps.to_string(),
            ])
            .args(["-i", "pipe:0", "-an", "-c:v", codec]);
        if codec.contains("videotoolbox") {
            cmd.args(["-realtime", "1", "-b:v", "2500k"]);
        } else if codec == "libx264" {
            cmd.args(["-preset", "veryfast", "-crf", "23"]);
        }
        cmd.args(["-pix_fmt", "yuv420p", "-g", &(fps * 2).to_string()])
            .args(["-movflags", "+frag_keyframe+empty_moov+default_base_moof"])
            .args(["-f", "mp4"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|source| sakshi_media::MediaError::Spawn {
                path: ffmpeg.path().to_path_buf(),
                source,
            })?;
        let stdin = child.stdin.take();
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Ok(Self {
            child,
            stdin,
            stderr: Some(stderr),
            width,
            height,
            path: path.to_path_buf(),
        })
    }

    fn write(&mut self, bgra: &[u8]) -> Result<(), CaptureError> {
        let stdin = self.stdin.as_mut().expect("open until finish");
        stdin
            .write_all(bgra)
            .map_err(|e| CaptureError::Encoder(format!("ffmpeg stopped accepting frames: {e}")))
    }

    /// Closes the input and waits for ffmpeg to finish the file.
    fn finish(mut self) -> Result<(), CaptureError> {
        drop(self.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|e| CaptureError::io(&self.path, e))?;
        let stderr = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if !status.success() {
            return Err(CaptureError::Encoder(format!(
                "ffmpeg exited with {status}: {}",
                stderr.trim()
            )));
        }
        Ok(())
    }
}

/// Feeds the encoder at a fixed frame rate until stopped.
pub struct VideoRecorder {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<u64, CaptureError>>,
}

impl VideoRecorder {
    /// Frame 0 is written at `started`; frame `n` at `started + n / fps`.
    pub fn start(
        mut encoder: VideoEncoder,
        slot: Arc<FrameSlot>,
        fps: u32,
        started: Instant,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("sakshi-video-ticker".into())
            .spawn(move || {
                let (w, h) = (encoder.width, encoder.height);
                let black: Vec<u8> = [0u8, 0, 0, 255].repeat((w * h) as usize);
                let tick = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
                let mut written = 0u64;
                let mut warned = false;
                while !stopping.load(Ordering::Acquire) {
                    let due = started + tick * u32::try_from(written).unwrap_or(u32::MAX);
                    let now = Instant::now();
                    if due > now {
                        std::thread::sleep(due - now);
                    } else if now - due > Duration::from_secs(2) && !warned {
                        tracing::warn!("video encoding is falling behind real time");
                        warned = true;
                    }
                    match slot.latest() {
                        Some(f) if f.width == w && f.height == h => encoder.write(&f.bgra)?,
                        _ => encoder.write(&black)?,
                    }
                    written += 1;
                }
                encoder.finish()?;
                Ok(written)
            })
            .expect("spawning the video ticker thread");
        Self { stop, thread }
    }

    /// Stops after the current frame and finishes the file; returns the
    /// number of frames written.
    pub fn stop(self) -> Result<u64, CaptureError> {
        self.stop.store(true, Ordering::Release);
        self.thread
            .join()
            .map_err(|_| CaptureError::Platform("the video thread panicked".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_frames(ffmpeg: &Ffmpeg, path: &Path) -> usize {
        let out = ffmpeg
            .command()
            .arg("-i")
            .arg(path)
            .args(["-map", "0:v", "-f", "framecrc", "-"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .count()
    }

    #[test]
    fn writes_one_frame_per_tick() {
        let ffmpeg = Ffmpeg::locate().expect("tests need ffmpeg");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video.mp4");
        let encoder = VideoEncoder::spawn(&ffmpeg, &path, 64, 48, 20, "mpeg4").unwrap();
        let slot = Arc::new(FrameSlot::default());
        let started = Instant::now();
        let recorder = VideoRecorder::start(encoder, Arc::clone(&slot), 20, started);

        // A frame of the wrong size is ignored (black is written instead),
        // then real frames arrive.
        slot.put(VideoFrame {
            width: 32,
            height: 32,
            bgra: Arc::new(vec![255; 32 * 32 * 4]),
        });
        std::thread::sleep(Duration::from_millis(200));
        slot.put(VideoFrame {
            width: 64,
            height: 48,
            bgra: Arc::new(vec![200; 64 * 48 * 4]),
        });
        std::thread::sleep(Duration::from_millis(800));
        let frames = recorder.stop().unwrap();
        let elapsed = started.elapsed().as_secs_f64();

        // One frame per 50 ms tick, give or take scheduling.
        let expected = (elapsed * 20.0) as u64;
        assert!(
            frames.abs_diff(expected) <= 2,
            "{frames} frames in {elapsed:.2}s"
        );
        assert_eq!(count_frames(&ffmpeg, &path) as u64, frames);
    }

    #[test]
    fn reports_a_failing_encoder() {
        let ffmpeg = Ffmpeg::locate().expect("tests need ffmpeg");
        let dir = tempfile::tempdir().unwrap();
        let encoder = VideoEncoder::spawn(
            &ffmpeg,
            &dir.path().join("v.mp4"),
            64,
            48,
            10,
            "no_such_codec",
        )
        .unwrap();
        let recorder = VideoRecorder::start(encoder, Arc::default(), 10, Instant::now());
        std::thread::sleep(Duration::from_millis(300));
        let err = recorder.stop().unwrap_err();
        assert!(matches!(err, CaptureError::Encoder(_)), "{err}");
    }
}
