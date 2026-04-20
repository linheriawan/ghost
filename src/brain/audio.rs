//! Audio I/O — microphone capture and speaker playback.
//!
//! Playback: rodio SamplesBuffer (24 kHz mono f32) via the default output.
//! Capture:  cpal default input device at 16 kHz mono for STT pipeline.

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// Play 32-bit float PCM samples on the default audio output.
///
/// Spawns a detached thread so it does not block the caller.
pub fn play_samples_async(samples: Vec<f32>, sample_rate: u32) {
    std::thread::Builder::new()
        .name("tts-playback".into())
        .spawn(move || match play_samples_blocking(&samples, sample_rate) {
            Ok(()) => log::info!("audio: playback finished"),
            Err(e) => log::error!("audio: playback error: {e}"),
        })
        .expect("failed to spawn playback thread");
}

/// Play samples synchronously — blocks until audio completes.
pub fn play_samples_blocking(samples: &[f32], sample_rate: u32) -> anyhow::Result<()> {
    use rodio::buffer::SamplesBuffer;
    use rodio::{OutputStream, Sink};

    let (_stream, handle) =
        OutputStream::try_default().map_err(|e| anyhow::anyhow!("audio output: {e}"))?;
    let sink = Sink::try_new(&handle).map_err(|e| anyhow::anyhow!("audio sink: {e}"))?;

    // channels: 1 (mono), sample_rate, data
    let source = SamplesBuffer::new(1u16, sample_rate, samples.to_vec());
    sink.append(source);
    sink.sleep_until_end();

    Ok(())
}

// ---------------------------------------------------------------------------
// VAD — simple energy-based voice activity detection
// ---------------------------------------------------------------------------

/// Events produced by VadState::feed().
pub enum VadEvent {
    /// Chunk was silence, nothing changed.
    Silence,
    /// Speech just started this chunk.
    SpeechStart,
    /// Still speaking.
    Speaking,
    /// Speech ended; the complete utterance is returned.
    UtteranceEnd(Vec<f32>),
}

/// VAD state for energy-based utterance detection.
pub struct VadState {
    /// RMS energy threshold — above this is considered speech.
    pub threshold: f32,
    /// Number of silent SAMPLES before an utterance is considered done.
    /// At 16 kHz: 9600 = 600 ms.
    pub silence_samples: usize,
    silent_sample_count: usize,
    has_speech: bool,
    buffer: Vec<f32>,
}

impl VadState {
    pub fn new(threshold: f32, silence_samples: usize) -> Self {
        Self {
            threshold,
            silence_samples,
            silent_sample_count: 0,
            has_speech: false,
            buffer: Vec::new(),
        }
    }

    /// Feed a chunk of 16 kHz mono samples, returns a VadEvent.
    pub fn feed(&mut self, chunk: &[f32]) -> VadEvent {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
        let is_speech = rms > self.threshold;

        if is_speech {
            let was_silent = !self.has_speech;
            self.has_speech = true;
            self.silent_sample_count = 0;
            self.buffer.extend_from_slice(chunk);
            if was_silent { VadEvent::SpeechStart } else { VadEvent::Speaking }
        } else if self.has_speech {
            // Still in post-speech tail — accumulate and track silence duration
            self.buffer.extend_from_slice(chunk);
            self.silent_sample_count += chunk.len();
            if self.silent_sample_count >= self.silence_samples {
                let utterance = std::mem::take(&mut self.buffer);
                self.has_speech = false;
                self.silent_sample_count = 0;
                VadEvent::UtteranceEnd(utterance)
            } else {
                VadEvent::Speaking
            }
        } else {
            VadEvent::Silence
        }
    }
}

// ---------------------------------------------------------------------------
// Microphone capture
// ---------------------------------------------------------------------------

/// Handle to a running microphone capture session.
///
/// Drop to stop capture (sends a stop signal to the capture thread).
pub struct AudioCapture {
    stop_tx: std::sync::mpsc::SyncSender<()>,
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

/// Start microphone capture using the device's native sample rate.
///
/// Audio is automatically downmixed to mono and resampled to 16 kHz before
/// being sent on `chunk_tx`, so Whisper always receives 16 kHz mono f32 PCM.
///
/// The returned `AudioCapture` keeps the stream alive — drop it to stop.
pub fn start_capture(
    chunk_tx: std::sync::mpsc::Sender<Vec<f32>>,
) -> anyhow::Result<AudioCapture> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no microphone found"))?;

    // Use whatever config the device actually supports.
    let supported = device
        .default_input_config()
        .map_err(|e| anyhow::anyhow!("cannot get input config: {e}"))?;

    let native_rate = supported.sample_rate().0;
    let native_channels = supported.channels() as usize;
    log::info!(
        "audio capture: device={}, native={}Hz {}ch",
        device.name().unwrap_or_default(),
        native_rate,
        native_channels
    );

    let config = supported.into();  // StreamConfig from SupportedStreamConfig

    const TARGET_RATE: u32 = 16_000;

    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(0);

    std::thread::Builder::new()
        .name("mic-capture".into())
        .spawn(move || {
            let tx = chunk_tx;

            let stream = match device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // 1. Downmix to mono
                    let mono: Vec<f32> = if native_channels == 1 {
                        data.to_vec()
                    } else {
                        data.chunks(native_channels)
                            .map(|frame| frame.iter().sum::<f32>() / native_channels as f32)
                            .collect()
                    };

                    // 2. Resample to 16 kHz (linear interpolation — sufficient for speech)
                    let resampled = if native_rate == TARGET_RATE {
                        mono
                    } else {
                        resample_linear(&mono, native_rate, TARGET_RATE)
                    };

                    let _ = tx.send(resampled);
                },
                |err| log::error!("audio capture error: {err}"),
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("mic-capture: build_input_stream failed: {e}");
                    return;
                }
            };

            if let Err(e) = stream.play() {
                log::error!("mic-capture: stream.play() failed: {e}");
                return;
            }

            log::info!("mic-capture: started (native {}Hz → 16kHz mono)", native_rate);
            let _ = stop_rx.recv();
            log::info!("mic-capture: stopped");
        })
        .expect("failed to spawn mic-capture thread");

    Ok(AudioCapture { stop_tx })
}

/// Linear interpolation resampler — adequate for speech intelligibility.
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let s0 = input.get(idx).copied().unwrap_or(0.0);
        let s1 = input.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + frac * (s1 - s0));
    }

    out
}
