//! Writing an audio track aligned to the session clock.
//!
//! Capture callbacks deliver buffers stamped with host-clock times. The
//! writer places each buffer at its time relative to the session start:
//! a late first buffer or a dropout becomes silence, and overlap is dropped,
//! so sample `n` of the file is always `n / rate` seconds into the session.
//! The WAV header is refreshed every second, so a crash leaves a playable
//! file.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::CaptureError;

/// Timing jitter ignored when placing buffers: 2 ms.
const JITTER_S: f64 = 0.002;
const FLUSH_EVERY: Duration = Duration::from_secs(1);

pub struct AudioTrackWriter {
    path: PathBuf,
    writer: hound::WavWriter<BufWriter<File>>,
    rate: u32,
    session_start_s: f64,
    written: u64,
    last_flush: Instant,
}

impl AudioTrackWriter {
    /// A mono 16-bit WAV at `rate` Hz whose first sample is at
    /// `session_start_s` on the host clock.
    pub fn create(path: &Path, rate: u32, session_start_s: f64) -> Result<Self, CaptureError> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec).map_err(|source| CaptureError::Wav {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            writer,
            rate,
            session_start_s,
            written: 0,
            last_flush: Instant::now(),
        })
    }

    fn wav_err(&self, source: hound::Error) -> CaptureError {
        CaptureError::Wav {
            path: self.path.clone(),
            source,
        }
    }

    /// Adds mono samples whose first one was captured at `pts_s`.
    pub fn push(&mut self, pts_s: f64, samples: &[f32]) -> Result<(), CaptureError> {
        let offset_s = pts_s - self.session_start_s;
        let at = (offset_s * f64::from(self.rate)).round() as i64;
        let jitter = (JITTER_S * f64::from(self.rate)) as i64;
        let written = i64::try_from(self.written).unwrap_or(i64::MAX);

        let mut samples = samples;
        if at > written + jitter {
            // A gap (or a late start): fill it with silence.
            for _ in written..at {
                self.writer
                    .write_sample(0i16)
                    .map_err(|e| self.wav_err(e))?;
            }
            self.written = at as u64;
        } else if at < written - jitter {
            // Overlap, or audio from before the session started: drop it.
            let skip = usize::try_from(written - at).unwrap_or(usize::MAX);
            samples = samples.get(skip..).unwrap_or_default();
        }
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            self.writer.write_sample(v).map_err(|e| self.wav_err(e))?;
        }
        self.written += samples.len() as u64;

        if self.last_flush.elapsed() >= FLUSH_EVERY {
            self.writer.flush().map_err(|e| self.wav_err(e))?;
            self.last_flush = Instant::now();
        }
        Ok(())
    }

    /// Finalizes the file; returns the number of samples written.
    pub fn finish(self) -> Result<u64, CaptureError> {
        let written = self.written;
        let path = self.path.clone();
        self.writer
            .finalize()
            .map_err(|source| CaptureError::Wav { path, source })?;
        Ok(written)
    }
}

/// Downmixes planar or interleaved multi-channel audio to mono.
pub fn downmix(channels: &[&[f32]]) -> Vec<f32> {
    match channels {
        [] => Vec::new(),
        [mono] => mono.to_vec(),
        many => {
            let len = many.iter().map(|c| c.len()).min().unwrap_or(0);
            let scale = 1.0 / many.len() as f32;
            (0..len)
                .map(|i| many.iter().map(|c| c[i]).sum::<f32>() * scale)
                .collect()
        }
    }
}

/// A buffer of mono samples captured at `pts_s` on the host clock.
pub struct AudioChunk {
    pub pts_s: f64,
    pub samples: Vec<f32>,
}

/// Owns a track writer on its own thread, so capture callbacks only send.
pub struct AudioRecorder {
    tx: mpsc::Sender<AudioChunk>,
    thread: JoinHandle<Result<u64, CaptureError>>,
}

impl AudioRecorder {
    pub fn start(writer: AudioTrackWriter) -> Self {
        let (tx, rx) = mpsc::channel::<AudioChunk>();
        let thread = std::thread::Builder::new()
            .name("sakshi-audio-writer".into())
            .spawn(move || {
                let mut writer = writer;
                for chunk in rx {
                    writer.push(chunk.pts_s, &chunk.samples)?;
                }
                writer.finish()
            })
            .expect("spawning the audio writer thread");
        Self { tx, thread }
    }

    /// A handle for capture callbacks.
    pub fn sender(&self) -> mpsc::Sender<AudioChunk> {
        self.tx.clone()
    }

    /// Waits for queued audio to be written once every sender is dropped.
    pub fn finish(self) -> Result<u64, CaptureError> {
        drop(self.tx);
        self.thread
            .join()
            .map_err(|_| CaptureError::Platform("the audio writer thread panicked".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> Vec<i16> {
        hound::WavReader::open(path)
            .unwrap()
            .into_samples::<i16>()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn aligns_buffers_to_the_session_clock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        // 1 kHz keeps the arithmetic readable: one sample per millisecond.
        let mut w = AudioTrackWriter::create(&path, 1000, 100.0).unwrap();

        w.push(100.010, &[0.5; 5]).unwrap(); // starts 10 ms late: 10 samples of silence
        w.push(100.015, &[0.5; 5]).unwrap(); // contiguous
        w.push(100.030, &[1.0; 3]).unwrap(); // 10 ms dropout: silence
        w.push(100.029, &[0.25; 6]).unwrap(); // overlaps by 4: those are dropped
        assert_eq!(w.finish().unwrap(), 35);

        let s = read(&path);
        let full = i16::MAX;
        let half = (f32::from(full) * 0.5).round() as i16;
        assert_eq!(&s[..10], &[0; 10]);
        assert_eq!(&s[10..20], &[half; 10]);
        assert_eq!(&s[20..30], &[0; 10]);
        assert_eq!(&s[30..33], &[full; 3]);
        let quarter = (f32::from(full) * 0.25).round() as i16;
        assert_eq!(&s[33..], &[quarter; 2]);
    }

    #[test]
    fn drops_audio_from_before_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = AudioTrackWriter::create(&path, 1000, 10.0).unwrap();

        w.push(9.995, &[0.5; 10]).unwrap();

        assert_eq!(w.finish().unwrap(), 5);
        assert_eq!(read(&path).len(), 5);
    }

    #[test]
    fn small_jitter_is_absorbed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = AudioTrackWriter::create(&path, 1000, 0.0).unwrap();
        w.push(0.0, &[0.1; 10]).unwrap();
        w.push(0.011, &[0.1; 10]).unwrap(); // 1 ms late: within jitter
        assert_eq!(w.finish().unwrap(), 20);
    }

    #[test]
    fn a_crash_after_a_flush_leaves_a_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = AudioTrackWriter::create(&path, 1000, 0.0).unwrap();
        w.push(0.0, &[0.2; 100]).unwrap();
        w.writer.flush().unwrap();
        // Simulate a crash: the writer is never finalized.
        std::mem::forget(w);

        assert_eq!(read(&path).len(), 100);
    }

    #[test]
    fn recorder_writes_on_its_own_thread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let recorder = AudioRecorder::start(AudioTrackWriter::create(&path, 1000, 0.0).unwrap());
        let tx = recorder.sender();
        std::thread::spawn(move || {
            for i in 0..10 {
                tx.send(AudioChunk {
                    pts_s: f64::from(i) * 0.01,
                    samples: vec![0.1; 10],
                })
                .unwrap();
            }
        })
        .join()
        .unwrap();
        assert_eq!(recorder.finish().unwrap(), 100);
    }

    #[test]
    fn downmixes_channels() {
        assert_eq!(downmix(&[&[1.0, 0.0], &[0.0, 0.0]]), [0.5, 0.0]);
        assert_eq!(downmix(&[&[0.3]]), [0.3]);
        assert!(downmix(&[]).is_empty());
    }
}
