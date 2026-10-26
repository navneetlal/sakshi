//! The microphone, through AVAudioEngine's input node.

use std::ptr::NonNull;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};

use crate::{AudioChunk, CaptureError};

/// Frames per tap callback (about 85 ms at 48 kHz).
const TAP_FRAMES: u32 = 4096;

pub(super) struct Microphone {
    engine: Retained<AVAudioEngine>,
    rate: u32,
    sender: Arc<Mutex<Option<Sender<AudioChunk>>>>,
}

impl Microphone {
    /// Opens the default input device without starting it.
    pub fn open() -> Result<Self, CaptureError> {
        let engine = unsafe { AVAudioEngine::new() };
        let format = unsafe { engine.inputNode().outputFormatForBus(0) };
        let (rate, channels) = unsafe { (format.sampleRate(), format.channelCount()) };
        if rate <= 0.0 || channels == 0 {
            return Err(CaptureError::Platform(
                "no microphone is available (or Microphone access is off)".into(),
            ));
        }
        Ok(Self {
            engine,
            rate: rate.round() as u32,
            sender: Arc::default(),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Starts delivering mono audio, stamped with host time, to `tx`.
    pub fn start(&self, tx: Sender<AudioChunk>) -> Result<(), CaptureError> {
        *self.sender.lock().expect("lock") = Some(tx);
        let sender = Arc::clone(&self.sender);
        let tap = RcBlock::new(
            move |buffer: NonNull<AVAudioPCMBuffer>, when: NonNull<AVAudioTime>| {
                // SAFETY: AVAudioEngine passes valid buffer and time objects that
                // live for the duration of the callback.
                let (buffer, when) = unsafe { (buffer.as_ref(), when.as_ref()) };
                if !unsafe { when.isHostTimeValid() } {
                    return;
                }
                let pts_s = unsafe { AVAudioTime::secondsForHostTime(when.hostTime()) };
                let Some(samples) = mono_samples(buffer) else {
                    return;
                };
                if let Some(tx) = sender.lock().expect("lock").as_ref() {
                    let _ = tx.send(AudioChunk { pts_s, samples });
                }
            },
        );
        unsafe {
            let input = self.engine.inputNode();
            input.installTapOnBus_bufferSize_format_block(
                0,
                TAP_FRAMES,
                None,
                RcBlock::as_ptr(&tap),
            );
            self.engine.prepare();
            self.engine.startAndReturnError()
        }
        .map_err(|e| {
            CaptureError::Platform(format!(
                "could not start the microphone: {}",
                e.localizedDescription()
            ))
        })
    }

    /// Stops the device and releases the sender.
    pub fn stop(&self) {
        unsafe {
            self.engine.inputNode().removeTapOnBus(0);
            self.engine.stop();
        }
        self.sender.lock().expect("lock").take();
    }
}

/// The buffer's float samples, averaged across channels.
fn mono_samples(buffer: &AVAudioPCMBuffer) -> Option<Vec<f32>> {
    let frames = unsafe { buffer.frameLength() } as usize;
    let data = unsafe { buffer.floatChannelData() };
    if frames == 0 || data.is_null() {
        return None;
    }
    let format = unsafe { buffer.format() };
    let channels = unsafe { format.channelCount() } as usize;
    if unsafe { format.isInterleaved() } {
        // One buffer of `frames * channels` interleaved samples.
        let all = unsafe { std::slice::from_raw_parts((*data).as_ptr(), frames * channels) };
        Some(
            all.chunks(channels.max(1))
                .map(|f| f.iter().sum::<f32>() / channels.max(1) as f32)
                .collect(),
        )
    } else {
        // One buffer of `frames` samples per channel.
        let planes: Vec<&[f32]> = (0..channels)
            .map(|c| unsafe { std::slice::from_raw_parts((*data.add(c)).as_ptr(), frames) })
            .collect();
        Some(crate::downmix(&planes))
    }
}
