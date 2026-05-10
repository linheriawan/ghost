//! Audio I/O — microphone capture and speaker playback.
//!
//! Playback: rodio SamplesBuffer (24 kHz mono f32) via the default output.
//! Capture:  cpal default input device at 16 kHz mono for STT pipeline.

// ---------------------------------------------------------------------------
// Playback — persistent sink (no click between segments)
// ---------------------------------------------------------------------------
//
// Each call to play_samples_async() previously created a new OutputStream,
// which caused an audible click/pop as the OS audio device was released and
// re-acquired.  PlaybackSink keeps one OutputStream open for the lifetime of
// the brain and queues samples onto a single Sink — transitions are seamless.

/// A long-lived audio output context.  Create once; call `play()` many times.
pub struct PlaybackSink {
    tx: std::sync::mpsc::Sender<(Vec<f32>, u32)>,
}

impl PlaybackSink {
    /// Spawn the background playback thread and return a handle.
    pub fn new() -> anyhow::Result<Self> {
        use rodio::buffer::SamplesBuffer;
        use rodio::{OutputStream, Sink};

        // Channel for (samples, sample_rate) pairs.
        let (tx, rx) = std::sync::mpsc::channel::<(Vec<f32>, u32)>();

        std::thread::Builder::new()
            .name("tts-playback".into())
            .spawn(move || {
                // OutputStream must live for the entire thread — dropping it
                // closes the device and causes the click we are trying to avoid.
                let (_stream, handle) = match OutputStream::try_default() {
                    Ok(v) => v,
                    Err(e) => { log::error!("audio: output device: {e}"); return; }
                };
                let sink = match Sink::try_new(&handle) {
                    Ok(s) => s,
                    Err(e) => { log::error!("audio: sink: {e}"); return; }
                };

                while let Ok((samples, sample_rate)) = rx.recv() {
                    // Append to the already-open sink — no device open/close, no click.
                    sink.append(SamplesBuffer::new(1u16, sample_rate, samples));
                    log::info!("audio: queued {} samples at {}Hz", sink.len(), sample_rate);
                }

                log::info!("audio: playback thread exiting");
            })
            .expect("failed to spawn tts-playback thread");

        Ok(Self { tx })
    }

    /// Queue samples for playback (non-blocking).  Returns estimated duration.
    pub fn play(&self, samples: Vec<f32>, sample_rate: u32) -> std::time::Duration {
        let secs = samples.len() as f64 / sample_rate as f64;
        let _ = self.tx.send((samples, sample_rate));
        std::time::Duration::from_secs_f64(secs)
    }
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
