use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read};
use std::path::Path;
use std::process::Stdio;

use image::RgbImage;
use image::codecs::jpeg::JpegEncoder;
use image_hasher::{HashAlg, Hasher, HasherConfig};
use sakshi_core::{Frame, MeetingDir};

use crate::{Ffmpeg, MediaError};

/// Largest frame accepted from ffmpeg, as a guard against a corrupt stream.
const MAX_PIXELS: usize = 8192 * 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyframeConfig {
    /// How often the video is sampled.
    pub sample_interval_ms: u64,
    /// Kept frames are downscaled to at most this width.
    pub max_width: u32,
    pub jpeg_quality: u8,
    /// Bits (of 64) by which a frame's perceptual hash must differ from the
    /// last kept frame to count as a change.
    pub change_threshold: u32,
    /// Changes closer than this to the last kept frame wait, so animations
    /// and transitions don't produce bursts of frames.
    pub min_gap_ms: u64,
    /// A frame is kept at least this often even if nothing changes.
    pub max_gap_ms: u64,
}

impl Default for KeyframeConfig {
    fn default() -> Self {
        Self {
            sample_interval_ms: 1000,
            max_width: 1280,
            jpeg_quality: 80,
            change_threshold: 10,
            min_gap_ms: 2000,
            max_gap_ms: 45_000,
        }
    }
}

/// Decides which sampled frames to keep, from their perceptual hashes.
#[derive(Debug, Clone)]
pub struct KeyframeSelector {
    change_threshold: u32,
    min_gap_ms: u64,
    max_gap_ms: u64,
    last_kept: Option<(u64, u64)>,
}

impl KeyframeSelector {
    pub fn new(config: &KeyframeConfig) -> Self {
        Self {
            change_threshold: config.change_threshold,
            min_gap_ms: config.min_gap_ms,
            max_gap_ms: config.max_gap_ms,
            last_kept: None,
        }
    }

    /// Returns whether the frame at `at_ms` with `hash` should be kept.
    /// Frames must be offered in time order.
    pub fn offer(&mut self, at_ms: u64, hash: u64) -> bool {
        let keep = match self.last_kept {
            None => true,
            Some((t, h)) => {
                let gap = at_ms.saturating_sub(t);
                gap >= self.max_gap_ms
                    || (gap >= self.min_gap_ms && hamming(h, hash) >= self.change_threshold)
            }
        };
        if keep {
            self.last_kept = Some((at_ms, hash));
        }
        keep
    }
}

/// Number of differing bits between two hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

impl Ffmpeg {
    /// Samples `video`, keeps frames that change meaningfully (plus one at
    /// least every `max_gap_ms`), and writes them as `<ms>.jpg` into
    /// `frames_dir`. Returns the kept frames in time order.
    pub fn extract_keyframes(
        &self,
        video: &Path,
        frames_dir: &Path,
        config: &KeyframeConfig,
    ) -> Result<Vec<Frame>, MediaError> {
        std::fs::create_dir_all(frames_dir).map_err(|e| MediaError::io(frames_dir, e))?;
        let interval = config.sample_interval_ms.max(1);
        let filter = format!(
            "fps=fps=1000/{interval},scale='min({w},iw)':-2",
            w = config.max_width
        );
        let mut child = self
            .command()
            .arg("-i")
            .arg(video)
            .args(["-an", "-vf", &filter])
            .args(["-f", "image2pipe", "-c:v", "ppm", "pipe:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| MediaError::Spawn {
                path: self.path().to_path_buf(),
                source,
            })?;
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });

        let hasher = perceptual_hasher();
        let mut selector = KeyframeSelector::new(config);
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut kept = Vec::new();
        let mut result = Ok(());
        for index in 0u64.. {
            let image = match read_ppm(&mut stdout) {
                Ok(Some(image)) => image,
                Ok(None) => break,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            let at_ms = index * interval;
            if selector.offer(at_ms, phash(&hasher, &image)) {
                let file = MeetingDir::frame_file(at_ms);
                let path = frames_dir.join(format!("{at_ms}.jpg"));
                if let Err(e) = write_jpeg(&path, &image, config.jpeg_quality) {
                    result = Err(e);
                    break;
                }
                kept.push(Frame {
                    at_ms,
                    file,
                    width: image.width(),
                    height: image.height(),
                });
            }
        }
        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|source| MediaError::Spawn {
            path: self.path().to_path_buf(),
            source,
        })?;
        let stderr = stderr_reader.join().unwrap_or_default();
        result?;
        if !status.success() {
            return Err(MediaError::FfmpegFailed {
                input: video.to_path_buf(),
                status: status.to_string(),
                stderr: stderr.trim().to_owned(),
            });
        }
        tracing::info!(kept = kept.len(), "extracted key frames");
        Ok(kept)
    }
}

/// 64-bit DCT perceptual hash (pHash).
fn perceptual_hasher() -> Hasher {
    HasherConfig::new()
        .hash_size(8, 8)
        .preproc_dct()
        .hash_alg(HashAlg::Mean)
        .to_hasher()
}

fn phash(hasher: &Hasher, image: &RgbImage) -> u64 {
    let hash = hasher.hash_image(image);
    let mut bytes = [0u8; 8];
    for (dst, src) in bytes.iter_mut().zip(hash.as_bytes()) {
        *dst = *src;
    }
    u64::from_le_bytes(bytes)
}

fn write_jpeg(path: &Path, image: &RgbImage, quality: u8) -> Result<(), MediaError> {
    let file = File::create(path).map_err(|e| MediaError::io(path, e))?;
    let mut out = BufWriter::new(file);
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(image)
        .map_err(|source| MediaError::Jpeg {
            path: path.to_path_buf(),
            source,
        })
}

/// Reads one binary PPM (`P6`, 8-bit) image, or `None` at end of stream.
fn read_ppm(r: &mut impl BufRead) -> Result<Option<RgbImage>, MediaError> {
    let Some(magic) = read_token(r)? else {
        return Ok(None);
    };
    if magic != "P6" {
        return Err(MediaError::FrameStream(format!(
            "expected P6, got {magic:?}"
        )));
    }
    let mut number = |what: &str| -> Result<usize, MediaError> {
        read_token(r)?
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| MediaError::FrameStream(format!("bad PPM {what}")))
    };
    let (width, height, max) = (number("width")?, number("height")?, number("maxval")?);
    if max != 255 || width == 0 || height == 0 || width * height > MAX_PIXELS {
        return Err(MediaError::FrameStream(format!(
            "unsupported frame {width}x{height} maxval {max}"
        )));
    }
    let mut data = vec![0u8; width * height * 3];
    r.read_exact(&mut data)
        .map_err(|e| MediaError::FrameStream(format!("truncated frame: {e}")))?;
    let size = |n: usize| u32::try_from(n).expect("bounded by MAX_PIXELS");
    Ok(RgbImage::from_raw(size(width), size(height), data))
}

/// Reads a whitespace-delimited header token, consuming exactly one
/// trailing whitespace byte. Returns `None` at a clean end of stream.
fn read_token(r: &mut impl BufRead) -> Result<Option<String>, MediaError> {
    let mut token = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match r.read(&mut byte) {
            Ok(0) if token.is_empty() => return Ok(None),
            Ok(0) => break,
            Ok(_) if byte[0].is_ascii_whitespace() => {
                if token.is_empty() {
                    continue;
                }
                break;
            }
            Ok(_) if byte[0] == b'#' && token.is_empty() => {
                let mut comment = Vec::new();
                r.read_until(b'\n', &mut comment)
                    .map_err(|e| MediaError::FrameStream(e.to_string()))?;
            }
            Ok(_) => token.push(byte[0]),
            Err(e) => return Err(MediaError::FrameStream(e.to_string())),
        }
    }
    Ok(Some(String::from_utf8_lossy(&token).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::tests::ffmpeg;

    fn selector() -> KeyframeSelector {
        KeyframeSelector::new(&KeyframeConfig::default())
    }

    #[test]
    fn keeps_first_frame_and_real_changes() {
        let mut s = selector();
        let a = 0u64;
        let small = 0b111; // 3 bits
        let big = 0xFFFF; // 16 bits

        assert!(s.offer(0, a));
        assert!(!s.offer(1000, a));
        assert!(!s.offer(3000, a ^ small));
        assert!(s.offer(4000, a ^ big));
        assert!(!s.offer(5000, a ^ big));
    }

    #[test]
    fn waits_out_the_minimum_gap() {
        let mut s = selector();
        assert!(s.offer(0, 0));
        // A change one second later waits; it is caught at the next sample
        // after the minimum gap, still compared with the last kept frame.
        assert!(!s.offer(1000, u64::MAX));
        assert!(s.offer(2000, u64::MAX));
    }

    #[test]
    fn keeps_a_frame_every_max_gap() {
        let mut s = selector();
        let kept: Vec<u64> = (0..=100)
            .map(|i| i * 1000)
            .filter(|&t| s.offer(t, 42))
            .collect();
        assert_eq!(kept, [0, 45_000, 90_000]);
    }

    #[test]
    fn threshold_is_configurable() {
        let config = KeyframeConfig {
            change_threshold: 3,
            ..KeyframeConfig::default()
        };
        let mut s = KeyframeSelector::new(&config);
        assert!(s.offer(0, 0));
        assert!(s.offer(5000, 0b111));
    }

    #[test]
    fn parses_a_ppm_stream() {
        let mut stream = Vec::new();
        for shade in [10u8, 200] {
            stream.extend(b"P6\n2 1\n255\n");
            stream.extend([shade; 6]);
        }
        let mut r = &stream[..];

        let first = read_ppm(&mut r).unwrap().unwrap();
        let second = read_ppm(&mut r).unwrap().unwrap();

        assert_eq!(first.dimensions(), (2, 1));
        assert_eq!(first.get_pixel(1, 0).0, [10, 10, 10]);
        assert_eq!(second.get_pixel(0, 0).0, [200, 200, 200]);
        assert!(read_ppm(&mut r).unwrap().is_none());
    }

    #[test]
    fn rejects_bad_frames() {
        assert!(read_ppm(&mut &b"P5\n1 1\n255\n\0"[..]).is_err());
        assert!(read_ppm(&mut &b"P6\n2 2\n255\n\0\0"[..]).is_err());
        assert!(read_ppm(&mut &b"P6\n99999 99999\n255\n"[..]).is_err());
    }

    fn make_video(dir: &Path, inputs: &[&str]) -> std::path::PathBuf {
        let path = dir.join("video.mp4");
        let mut cmd = ffmpeg().command();
        for input in inputs {
            cmd.args(["-f", "lavfi", "-i", input]);
        }
        let streams: String = (0..inputs.len()).map(|i| format!("[{i}:v]")).collect();
        let status = cmd
            .args([
                "-filter_complex",
                &format!("{streams}concat=n={}:v=1[v]", inputs.len()),
            ])
            .args(["-map", "[v]", "-c:v", "mpeg4", "-q:v", "3"])
            // Cap the length in case a source has no end.
            .args(["-t", "120"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    #[test]
    fn extracts_scene_changes_from_video() {
        let dir = tempfile::tempdir().unwrap();
        let video = make_video(
            dir.path(),
            &[
                "smptebars=size=320x240:rate=5:duration=6",
                "testsrc=size=320x240:rate=5:duration=6",
                "rgbtestsrc=size=320x240:rate=5:duration=2",
            ],
        );
        let frames_dir = dir.path().join("frames");

        let frames = ffmpeg()
            .extract_keyframes(&video, &frames_dir, &KeyframeConfig::default())
            .unwrap();

        let times: Vec<u64> = frames.iter().map(|f| f.at_ms).collect();
        assert_eq!(times.first(), Some(&0), "{times:?}");
        assert!(
            times.iter().any(|&t| (6000..=8000).contains(&t)),
            "no frame near the 6 s cut: {times:?}"
        );
        assert!(times.len() <= 6, "too many frames: {times:?}");
        for f in &frames {
            assert_eq!((f.width, f.height), (320, 240));
            assert_eq!(f.file, format!("frames/{}.jpg", f.at_ms));
            assert!(frames_dir.join(format!("{}.jpg", f.at_ms)).is_file());
        }
    }

    #[test]
    fn downscales_wide_video_and_keeps_static_frames_every_max_gap() {
        let dir = tempfile::tempdir().unwrap();
        let video = make_video(
            dir.path(),
            &["color=c=gray:size=1920x1080:rate=2:duration=50"],
        );

        let frames = ffmpeg()
            .extract_keyframes(
                &video,
                &dir.path().join("frames"),
                &KeyframeConfig::default(),
            )
            .unwrap();

        let times: Vec<u64> = frames.iter().map(|f| f.at_ms).collect();
        assert_eq!(times, [0, 45_000]);
        assert_eq!((frames[0].width, frames[0].height), (1280, 720));
    }
}
