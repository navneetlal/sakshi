//! The ScreenCaptureKit stream: video frames into a [`FrameSlot`], app audio
//! into the system track.

use std::ptr::NonNull;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType,
};

use super::content::Target;
use crate::{AudioChunk, CaptureError, FrameSlot, VideoFrame};

/// Frames ScreenCaptureKit may hold before we take them.
const QUEUE_DEPTH: isize = 6;

struct OutputState {
    frames: Arc<FrameSlot>,
    /// Taken on stop, so the audio writer sees the end of its input.
    audio: Mutex<Option<Sender<AudioChunk>>>,
    interruption: Arc<Mutex<Option<String>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class
    // doesn't implement Drop.
    #[unsafe(super = NSObject)]
    #[name = "SakshiStreamOutput"]
    #[ivars = OutputState]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            let state = self.ivars();
            if kind == SCStreamOutputType::Screen {
                video_frame(buffer, &state.frames);
            } else if kind == SCStreamOutputType::Audio {
                audio_buffer(buffer, &state.audio);
            }
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            let message = error.localizedDescription().to_string();
            tracing::warn!(%message, "capture stopped by the system");
            *self.ivars().interruption.lock().expect("lock") = Some(message);
        }
    }
);

impl StreamOutput {
    fn new(state: OutputState) -> Retained<Self> {
        let this = Self::alloc().set_ivars(state);
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// Copies a BGRA frame out of the sample buffer. Idle frames (the screen
/// didn't change) carry no image and are skipped.
fn video_frame(buffer: &CMSampleBuffer, frames: &FrameSlot) {
    let Some(image) = (unsafe { buffer.image_buffer() }) else {
        return;
    };
    let pixels = &*image;
    if unsafe { CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) } != 0 {
        return;
    }
    let base = CVPixelBufferGetBaseAddress(pixels)
        .cast::<u8>()
        .cast_const();
    let stride = CVPixelBufferGetBytesPerRow(pixels);
    let (width, height) = (
        CVPixelBufferGetWidth(pixels),
        CVPixelBufferGetHeight(pixels),
    );
    if !base.is_null() && stride >= width * 4 {
        let mut bgra = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            // SAFETY: the locked buffer holds `height` rows of `stride`
            // bytes, each starting with `width * 4` bytes of pixels.
            let line = unsafe { std::slice::from_raw_parts(base.add(row * stride), width * 4) };
            bgra.extend_from_slice(line);
        }
        frames.put(VideoFrame {
            width: width as u32,
            height: height as u32,
            bgra: Arc::new(bgra),
        });
    }
    unsafe { CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) };
}

/// Sends the buffer's audio, downmixed to mono, stamped with its host time.
/// ScreenCaptureKit delivers non-interleaved float32.
fn audio_buffer(buffer: &CMSampleBuffer, sender: &Mutex<Option<Sender<AudioChunk>>>) {
    let Some(pts_s) = super::seconds(unsafe { buffer.presentation_time_stamp() }) else {
        return;
    };
    let frames = usize::try_from(unsafe { buffer.num_samples() }).unwrap_or(0);
    let Some(block) = (unsafe { buffer.data_buffer() }) else {
        return;
    };
    let bytes = unsafe { block.data_length() };
    if frames == 0 || bytes == 0 || bytes % (frames * 4) != 0 {
        return;
    }
    let mut data = vec![0f32; bytes / 4];
    let destination = NonNull::new(data.as_mut_ptr().cast()).expect("vec pointers are non-null");
    if unsafe { block.copy_data_bytes(0, bytes, destination) } != 0 {
        return;
    }
    let planes: Vec<&[f32]> = data.chunks(frames).collect();
    let samples = crate::downmix(&planes);
    if let Some(tx) = sender.lock().expect("lock").as_ref() {
        let _ = tx.send(AudioChunk { pts_s, samples });
    }
}

fn wait_for(rx: &mpsc::Receiver<Option<String>>, what: &str) -> Result<(), CaptureError> {
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(None) => Ok(()),
        Ok(Some(message)) => Err(CaptureError::Platform(format!(
            "could not {what}: {message}"
        ))),
        Err(_) => Err(CaptureError::Platform(format!(
            "ScreenCaptureKit did not {what} within 10 s"
        ))),
    }
}

/// A completion block and the receiver its error message (if any) arrives on.
type Completion = (
    RcBlock<dyn Fn(*mut NSError)>,
    mpsc::Receiver<Option<String>>,
);

/// A completion handler reporting an optional error over a channel.
fn completion() -> Completion {
    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |error: *mut NSError| {
        let message = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
        let _ = tx.try_send(message);
    });
    (block, rx)
}

pub(super) struct Stream {
    stream: Retained<SCStream>,
    output: Retained<StreamOutput>,
    interruption: Arc<Mutex<Option<String>>>,
    _queue: DispatchRetained<DispatchQueue>,
}

impl Stream {
    pub fn start(
        target: &Target,
        width: u32,
        height: u32,
        fps: u32,
        audio_rate: u32,
        frames: Arc<FrameSlot>,
        audio: Option<Sender<AudioChunk>>,
    ) -> Result<Self, CaptureError> {
        let capture_audio = audio.is_some();
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(width as usize);
            config.setHeight(height as usize);
            config.setMinimumFrameInterval(CMTime {
                value: 1,
                timescale: i32::try_from(fps.max(1)).unwrap_or(30),
                flags: CMTimeFlags::Valid,
                epoch: 0,
            });
            config.setPixelFormat(kCVPixelFormatType_32BGRA);
            config.setScalesToFit(true);
            config.setPreservesAspectRatio(true);
            config.setShowsCursor(true);
            config.setQueueDepth(QUEUE_DEPTH);
            config.setCapturesAudio(capture_audio);
            config.setSampleRate(audio_rate as isize);
            config.setChannelCount(1);
            config.setExcludesCurrentProcessAudio(true);
        }

        let interruption = Arc::new(Mutex::new(None));
        let output = StreamOutput::new(OutputState {
            frames,
            audio: Mutex::new(audio),
            interruption: Arc::clone(&interruption),
        });
        let delegate = ProtocolObject::<dyn SCStreamDelegate>::from_ref(&*output);
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &target.filter,
                &config,
                Some(delegate),
            )
        };
        let queue = DispatchQueue::new("app.sakshi.capture", None);
        let handler = ProtocolObject::<dyn SCStreamOutput>::from_ref(&*output);
        let mut kinds = vec![SCStreamOutputType::Screen];
        if capture_audio {
            kinds.push(SCStreamOutputType::Audio);
        }
        for kind in kinds {
            unsafe {
                stream.addStreamOutput_type_sampleHandlerQueue_error(handler, kind, Some(&queue))
            }
            .map_err(|e| {
                CaptureError::Platform(format!(
                    "could not add a stream output: {}",
                    e.localizedDescription()
                ))
            })?;
        }

        let (done, rx) = completion();
        unsafe { stream.startCaptureWithCompletionHandler(Some(&done)) };
        wait_for(&rx, "start capturing")?;
        Ok(Self {
            stream,
            output,
            interruption,
            _queue: queue,
        })
    }

    pub fn interruption(&self) -> Option<String> {
        self.interruption.lock().expect("lock").clone()
    }

    /// Seconds between the stream's clock and the host clock (expected ~0).
    pub fn clock_offset(&self, host_now: f64) -> Option<f64> {
        let clock = unsafe { self.stream.synchronizationClock() }?;
        super::seconds(unsafe { clock.time() }).map(|t| t - host_now)
    }

    /// Stops capture and releases the audio sender.
    pub fn stop(self) -> Result<(), CaptureError> {
        let result = if self.interruption().is_some() {
            Ok(())
        } else {
            let (done, rx) = completion();
            unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&done)) };
            wait_for(&rx, "stop capturing")
        };
        self.output.ivars().audio.lock().expect("lock").take();
        result
    }
}
