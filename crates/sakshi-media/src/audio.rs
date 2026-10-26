use std::path::Path;

use crate::MediaError;

/// The shape of a WAV file's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub float: bool,
    pub duration_ms: u64,
}

impl WavInfo {
    /// 16 kHz mono in a sample format nemo-speech reads directly (PCM16 or
    /// float32), so no conversion is needed.
    pub fn is_pipeline_ready(&self) -> bool {
        self.sample_rate == 16_000
            && self.channels == 1
            && ((!self.float && self.bits_per_sample == 16)
                || (self.float && self.bits_per_sample == 32))
    }
}

/// Reads a WAV header.
pub fn wav_info(path: &Path) -> Result<WavInfo, MediaError> {
    let reader = hound::WavReader::open(path).map_err(|source| MediaError::Wav {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    let frames = u64::from(reader.duration());
    Ok(WavInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bits_per_sample: spec.bits_per_sample,
        float: spec.sample_format == hound::SampleFormat::Float,
        duration_ms: (frames * 1000)
            .checked_div(u64::from(spec.sample_rate))
            .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, sample_rate: u32, channels: u16, float: bool, frames: u32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: if float { 32 } else { 16 },
            sample_format: if float {
                hound::SampleFormat::Float
            } else {
                hound::SampleFormat::Int
            },
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..frames * u32::from(channels) {
            if float {
                w.write_sample(0.0f32).unwrap();
            } else {
                w.write_sample(0i16).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn recognizes_pipeline_ready_audio() {
        let dir = tempfile::tempdir().unwrap();
        let cases = [
            (16_000, 1, false, true),
            (16_000, 1, true, true),
            (48_000, 1, false, false),
            (16_000, 2, false, false),
        ];
        for (rate, channels, float, ready) in cases {
            let path = dir.path().join(format!("{rate}-{channels}-{float}.wav"));
            write_wav(&path, rate, channels, float, rate / 2);
            let info = wav_info(&path).unwrap();
            assert_eq!(info.is_pipeline_ready(), ready, "{info:?}");
            assert_eq!(info.duration_ms, 500);
        }
    }

    #[test]
    fn rejects_non_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        std::fs::write(&path, b"not a wav").unwrap();
        assert!(matches!(wav_info(&path), Err(MediaError::Wav { .. })));
    }
}
