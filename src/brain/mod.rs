//! Brain module — LLM, TTS, STT coordinator
//!
//! Architecture:
//!
//! ```text
//! UI thread  ──BrainCommand──►  brain thread  ──InferCmd──►  inference thread
//!            ◄─BrainResponse──               ◄─InferRes───
//! ```
//!
//! - The inference thread loads the model once, then processes one query at a
//!   time via a rendezvous sync_channel(0).
//! - Tokens stream back immediately as `BrainResponse::ChatToken`.
//! - After ChatDone the brain thread synthesizes TTS (if configured) and sends
//!   `BrainResponse::SpeechReady` with raw PCM samples.

pub mod audio;
pub mod inference;
pub mod model_manager;
pub mod stt;
pub mod tts;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Public command / response types (UI ↔ brain thread)
// ---------------------------------------------------------------------------

/// Commands sent TO the brain from the UI thread.
#[derive(Debug)]
pub enum BrainCommand {
    /// Send a chat message and get a streaming response.
    Chat { message: String },
    /// Toggle voice mode on/off.
    SetVoiceMode(bool),
    /// Raw 16 kHz mono PCM chunk from the microphone (voice mode).
    AudioChunk(Vec<f32>),
}

/// Responses sent FROM the brain back to the UI thread.
#[derive(Debug)]
pub enum BrainResponse {
    /// A single streamed token (call UI repaint after each).
    ChatToken { token: String },
    /// Signals the end of one streaming LLM response.
    ChatDone,
    /// An error occurred while processing.
    Error { message: String },
    /// Draft text for the input box during the voice pipeline (raw STT → streaming correction).
    /// The UI replaces input_text with this on every update; does NOT auto-send.
    InputDraft { text: String },
    /// Final transcription after correction — UI auto-sends this as a user message.
    Transcription { text: String },
    /// Voice mode changed (true = listening, false = text mode).
    VoiceModeChanged(bool),
    /// Real-time voice pipeline status (shown in the chat window status bar).
    VoiceStatus(String),
}

// ---------------------------------------------------------------------------
// Internal inference-thread protocol
// ---------------------------------------------------------------------------

enum InferCmd {
    Run(String), // full ChatML prompt
    Stop,
}

enum InferRes {
    ModelReady,
    ModelFailed(String),
    Token(String),
    Done,
}

// ---------------------------------------------------------------------------
// Internal STT-thread protocol
// ---------------------------------------------------------------------------

/// Send an utterance buffer to the STT thread.
enum SttCmd {
    Transcribe(Vec<f32>),
    Stop,
}

/// Result from the STT thread back to the brain coordinator.
enum SttResult {
    Ok(String),
    Empty,
    Err(String),
}

// ---------------------------------------------------------------------------
// BrainService
// ---------------------------------------------------------------------------

use crate::config::BrainConfig;
use audio::VadEvent;

pub struct BrainService;

impl BrainService {
    /// Spawn the brain background thread.
    ///
    /// Returns a `Sender<BrainCommand>` the UI uses to send messages.
    /// Responses arrive on `response_tx` (pass the receiving end to the UI).
    pub fn spawn(
        config: BrainConfig,
        response_tx: Sender<BrainResponse>,
    ) -> Sender<BrainCommand> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<BrainCommand>();
        // Clone so the brain thread can self-send AudioChunk from the mic forwarder.
        let self_tx = cmd_tx.clone();

        std::thread::Builder::new()
            .name("brain".into())
            .spawn(move || {
                Self::run(config, cmd_rx, self_tx, response_tx);
            })
            .expect("failed to spawn brain thread");

        cmd_tx
    }

    fn run(
        config: BrainConfig,
        cmd_rx: Receiver<BrainCommand>,
        cmd_rx_to_self: Sender<BrainCommand>,
        response_tx: Sender<BrainResponse>,
    ) {
        let model_path = PathBuf::from(&config.llm_model);
        let n_gpu_layers = config.gpu_layers.unwrap_or(0);
        let _n_ctx = config.context_size.unwrap_or(4096);
        let system = config
            .system_prompt
            .clone()
            .unwrap_or_else(|| "You are a helpful assistant.".to_string());

        // Rendezvous channel (capacity 0 = one query at a time).
        let (infer_cmd_tx, infer_cmd_rx) = std::sync::mpsc::sync_channel::<InferCmd>(0);
        let (infer_res_tx, infer_res_rx) = std::sync::mpsc::channel::<InferRes>();

        // Spawn dedicated inference thread — loads model once, then loops.
        std::thread::Builder::new()
            .name("inference".into())
            .spawn(move || {
                inference_thread(model_path, n_gpu_layers, infer_cmd_rx, infer_res_tx);
            })
            .expect("failed to spawn inference thread");

        // Wait for model to finish loading.
        match infer_res_rx.recv() {
            Ok(InferRes::ModelReady) => {
                log::info!("Brain: model ready");
            }
            Ok(InferRes::ModelFailed(e)) => {
                log::error!("Brain: model failed to load: {e}");
                let _ = response_tx.send(BrainResponse::Error { message: e });
                return;
            }
            _ => {
                log::error!("Brain: inference thread exited unexpectedly during load");
                return;
            }
        }

        // Optional TTS
        let tts = load_tts(&config);
        if tts.is_some() {
            log::info!("Brain: TTS ready");
        } else {
            log::info!("Brain: TTS not configured or failed to load");
        }

        // Persistent playback sink — one OutputStream for the whole session.
        // Avoids the click/pop that occurred when open/closing per-segment.
        let playback = audio::PlaybackSink::new().ok();
        if playback.is_some() {
            log::info!("Brain: playback sink ready");
        } else {
            log::warn!("Brain: playback sink failed — TTS will be silent");
        }

        // Optional STT — spawned on a dedicated thread so Whisper never blocks
        // the brain coordinator.  The coordinator sends Vec<f32> utterances and
        // receives SttResult via separate channels.
        let (stt_cmd_tx, stt_result_rx) = {
            let stt_model = load_stt(&config);
            let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<SttCmd>();
            let (res_tx, res_rx) = std::sync::mpsc::channel::<SttResult>();

            if let Some(model) = stt_model {
                log::info!("Brain: STT ready — spawning dedicated thread");
                std::thread::Builder::new()
                    .name("stt".into())
                    .spawn(move || {
                        stt_thread(model, cmd_rx, res_tx);
                    })
                    .expect("failed to spawn stt thread");
            } else {
                log::info!("Brain: STT not configured");
                // Spawn a stub thread that just drains the channel.
                std::thread::Builder::new()
                    .name("stt-stub".into())
                    .spawn(move || {
                        while let Ok(cmd) = cmd_rx.recv() {
                            match cmd {
                                SttCmd::Stop => break,
                                SttCmd::Transcribe(_) => {
                                    let _ = res_tx.send(SttResult::Err("STT not configured".into()));
                                }
                            }
                        }
                    })
                    .expect("failed to spawn stt-stub thread");
            }

            (cmd_tx, res_rx)
        };

        let mut history: Vec<(String, String)> = Vec::new();
        let mut voice_mode = false;
        let mut mic_capture: Option<audio::AudioCapture> = None;

        // VAD: energy threshold (RMS), and silence duration before utterance ends.
        // Silence duration is tracked in samples (16 kHz), not frames, so it's
        // independent of the device buffer size.
        let mut vad = audio::VadState::new(0.05, 9_600); // 9600 samples = 600 ms at 16 kHz
        let mut chunk_count = 0u32;
        // Consecutive chunks where RMS < 0.0001 (effectively silence/zeros).
        // On macOS, mic permission denied returns all-zero samples.
        let mut zero_chunk_streak = 0u32;
        // Mic muting: suppress AudioChunk processing while TTS is playing to
        // prevent the speaker output from looping back into the STT pipeline.
        let mut mic_mute_until: Option<Instant> = None;

        log::info!("Brain: entering command loop");

        loop {
            // Poll stt_result_rx first (non-blocking) so a finished Whisper
            // transcription is handled immediately, even if a cmd is waiting.
            while let Ok(stt_res) = stt_result_rx.try_recv() {
                handle_stt_result(
                    stt_res,
                    &infer_cmd_tx,
                    &infer_res_rx,
                    &response_tx,
                    &mut mic_mute_until,
                );
            }

            // Block on the next brain command.
            let cmd = match cmd_rx.recv() {
                Ok(c) => c,
                Err(_) => break,
            };
            match cmd {
                BrainCommand::Chat { message } => {
                    log::info!("Brain: chat ({} chars)", message.len());

                    let prompt = inference::build_prompt(&system, &history, &message);

                    if infer_cmd_tx.send(InferCmd::Run(prompt)).is_err() {
                        log::error!("Brain: inference thread gone");
                        break;
                    }

                    // Forward streaming tokens to UI.
                    let mut response = String::new();
                    loop {
                        match infer_res_rx.recv() {
                            Ok(InferRes::Token(piece)) => {
                                response.push_str(&piece);
                                let _ = response_tx
                                    .send(BrainResponse::ChatToken { token: piece });
                            }
                            Ok(InferRes::Done) | Err(_) => break,
                            _ => {}
                        }
                    }

                    history.push((message, response.clone()));

                    // Keep at most the last 8 exchanges.
                    if history.len() > 8 {
                        history.drain(..history.len() - 8);
                    }

                    let _ = response_tx.send(BrainResponse::ChatDone);

                    // Synthesize TTS after ChatDone (so UI can update first).
                    // Skip TTS for CJK text — Kokoro can't handle it and egui
                    // can't render CJK glyphs anyway.
                    if let (Some(ref kokoro), Some(ref sink)) = (&tts, &playback) {
                        if contains_cjk(&response) {
                            log::info!("Brain: skipping TTS for CJK response");
                        } else {
                            match kokoro.synthesize(&response) {
                                Ok(samples) if !samples.is_empty() => {
                                    // Play via persistent sink — no click between segments.
                                    let duration = sink.play(samples, tts::SAMPLE_RATE);
                                    mic_mute_until = Some(
                                        Instant::now() + duration + Duration::from_millis(800),
                                    );
                                    log::info!("Brain: TTS queued, muting mic for {duration:.1?}");
                                }
                                Ok(_) => log::warn!("Brain: TTS produced empty audio"),
                                Err(e) => log::error!("Brain: TTS error: {e}"),
                            }
                        }
                    }
                }

                BrainCommand::SetVoiceMode(on) => {
                    voice_mode = on;

                    if on {
                        // Fresh VAD state each time voice mode is enabled so stale
                        // noise / stuck has_speech from a previous session can't block.
                        vad = audio::VadState::new(0.05, 9_600);
                        mic_mute_until = None;
                        chunk_count = 0;
                        zero_chunk_streak = 0;

                        // Start mic capture — chunks come back as AudioChunk commands.
                        let (chunk_tx, chunk_rx) = std::sync::mpsc::channel::<Vec<f32>>();

                        // Forward mic chunks back to ourselves via cmd_tx clone.
                        // We use a side-channel because cmd_rx is already being recv'd.
                        // Instead: forward directly to brain via a dedicated thread.
                        let brain_cmd_tx = cmd_rx_to_self.clone();
                        std::thread::Builder::new()
                            .name("mic-forwarder".into())
                            .spawn(move || {
                                while let Ok(chunk) = chunk_rx.recv() {
                                    if brain_cmd_tx.send(BrainCommand::AudioChunk(chunk)).is_err() {
                                        break;
                                    }
                                }
                                log::info!("mic-forwarder: channel closed");
                            })
                            .expect("failed to spawn mic-forwarder");

                        match audio::start_capture(chunk_tx) {
                            Ok(cap) => {
                                mic_capture = Some(cap);
                                log::info!("Brain: microphone capture started");
                            }
                            Err(e) => {
                                log::error!("Brain: mic capture failed: {e}");
                                let _ = response_tx.send(BrainResponse::Error {
                                    message: format!("Microphone error: {e}"),
                                });
                            }
                        }
                    } else {
                        // Stop mic capture by dropping the handle.
                        mic_capture = None;
                        log::info!("Brain: microphone capture stopped");
                    }

                    let _ = response_tx.send(BrainResponse::VoiceModeChanged(on));
                    log::info!("Brain: voice mode = {on}");
                }

                BrainCommand::AudioChunk(chunk) => {
                    if !voice_mode {
                        continue;
                    }

                    // Suppress mic input while TTS is playing to prevent echo feedback.
                    if let Some(until) = mic_mute_until {
                        if Instant::now() < until {
                            continue;
                        }
                        // Mute period ended — clear flag and reset VAD so stale
                        // speaker audio in the buffer doesn't trigger transcription.
                        mic_mute_until = None;
                        vad = audio::VadState::new(0.05, 9_600);
                        let _ = response_tx
                            .send(BrainResponse::VoiceStatus("Listening  ..........".to_string()));
                    }

                    // Calculate RMS of this chunk for the level indicator.
                    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
                    // Scale so speech (0.05–0.2 RMS) fills most of the bar.
                    let bars = (rms * 100.0).clamp(0.0, 10.0) as usize;
                    let level_str: String = "#".repeat(bars) + &".".repeat(10 - bars);

                    chunk_count = chunk_count.wrapping_add(1);

                    // Track how many consecutive chunks are near-zero.
                    // On macOS, mic permission denied → device returns all zeros.
                    if rms < 0.0005 {
                        zero_chunk_streak = zero_chunk_streak.saturating_add(1);
                    } else {
                        zero_chunk_streak = 0;
                    }

                    // Log raw RMS every ~50 chunks so the user can tail the log to debug.
                    if chunk_count % 50 == 0 {
                        log::info!("Brain: mic rms={:.5}  bars={}  zeros={}", rms, bars, zero_chunk_streak);
                    }

                    match vad.feed(&chunk) {
                        VadEvent::Silence => {
                            // Update status every 4 chunks so the bar visibly animates.
                            if chunk_count % 4 == 0 {
                                let status = if zero_chunk_streak > 200 {
                                    // ~200 chunks of silence ≈ 2–4 s with typical buffer sizes.
                                    // Very likely mic permission is denied.
                                    "No audio — grant mic permission in System Settings".to_string()
                                } else {
                                    format!("Listening  {level_str}")
                                };
                                let _ = response_tx.send(BrainResponse::VoiceStatus(status));
                            }
                        }
                        VadEvent::SpeechStart => {
                            let _ = response_tx.send(BrainResponse::VoiceStatus(
                                format!("Hearing you  {level_str}"),
                            ));
                        }
                        VadEvent::Speaking => {
                            if chunk_count % 4 == 0 {
                                let _ = response_tx.send(BrainResponse::VoiceStatus(
                                    format!("Hearing you  {level_str}"),
                                ));
                            }
                        }
                        VadEvent::UtteranceEnd(utterance) => {
                            log::info!("Brain: utterance end ({} samples) → STT thread", utterance.len());
                            let _ = response_tx.send(BrainResponse::VoiceStatus(
                                "Transcribing...".to_string(),
                            ));
                            // Send to dedicated STT thread — does NOT block the
                            // brain coordinator; result arrives via stt_result_rx.
                            let _ = stt_cmd_tx.send(SttCmd::Transcribe(utterance));
                        }
                    }
                }
            }
        }

        // Signal worker threads to exit.
        let _ = stt_cmd_tx.send(SttCmd::Stop);
        let _ = infer_cmd_tx.send(InferCmd::Stop);
        log::info!("Brain: command channel closed, shutting down");
    }
}

// ---------------------------------------------------------------------------
// Inference thread — lives for the entire session
// ---------------------------------------------------------------------------

fn inference_thread(
    model_path: PathBuf,
    n_gpu_layers: u32,
    cmd_rx: Receiver<InferCmd>,
    res_tx: Sender<InferRes>,
) {
    let session = match inference::InferenceSession::load(&model_path, n_gpu_layers) {
        Ok(s) => {
            res_tx.send(InferRes::ModelReady).ok();
            s
        }
        Err(e) => {
            log::error!("Inference: load failed: {e}");
            res_tx.send(InferRes::ModelFailed(e.to_string())).ok();
            return;
        }
    };

    log::info!("Inference thread: ready");

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            InferCmd::Run(prompt) => {
                let _ = session.run_streaming(&prompt, |piece| {
                    res_tx.send(InferRes::Token(piece)).ok();
                });
                res_tx.send(InferRes::Done).ok();
            }
            InferCmd::Stop => break,
        }
    }

    log::info!("Inference thread: exiting");
}

// ---------------------------------------------------------------------------
// STT thread — Whisper runs here, never blocks brain coordinator
// ---------------------------------------------------------------------------

fn stt_thread(
    model: stt::WhisperStt,
    cmd_rx: Receiver<SttCmd>,
    res_tx: Sender<SttResult>,
) {
    log::info!("STT thread: ready");
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            SttCmd::Stop => break,
            SttCmd::Transcribe(utterance) => {
                match model.transcribe(&utterance) {
                    Ok(text) if !text.trim().is_empty() => {
                        let _ = res_tx.send(SttResult::Ok(text.trim().to_string()));
                    }
                    Ok(_) => { let _ = res_tx.send(SttResult::Empty); }
                    Err(e) => { let _ = res_tx.send(SttResult::Err(e.to_string())); }
                }
            }
        }
    }
    log::info!("STT thread: exiting");
}

/// Called by the brain coordinator when a SttResult arrives on stt_result_rx.
/// Runs correction synchronously (blocks coordinator, but correction is short).
fn handle_stt_result(
    result: SttResult,
    infer_cmd_tx: &std::sync::mpsc::SyncSender<InferCmd>,
    infer_res_rx: &std::sync::mpsc::Receiver<InferRes>,
    response_tx: &Sender<BrainResponse>,
    mic_mute_until: &mut Option<Instant>,
) {
    match result {
        SttResult::Ok(raw) => {
            log::info!("Brain: STT result = {:?}", raw);
            let _ = response_tx.send(BrainResponse::InputDraft { text: raw.clone() });
            let _ = response_tx.send(BrainResponse::VoiceStatus("Correcting...".to_string()));
            let corrected = stream_correction(&raw, infer_cmd_tx, infer_res_rx, response_tx);
            log::info!("Brain: corrected = {:?}", corrected);
            let _ = response_tx.send(BrainResponse::Transcription { text: corrected });
        }
        SttResult::Empty => {
            log::debug!("Brain: STT empty");
            let _ = response_tx.send(BrainResponse::VoiceStatus("Listening  ..........".to_string()));
        }
        SttResult::Err(e) => {
            log::error!("Brain: STT error: {e}");
            let _ = response_tx.send(BrainResponse::VoiceStatus(format!("STT error: {e}")));
        }
    }
}

// ---------------------------------------------------------------------------
// TTS loader helper
// ---------------------------------------------------------------------------

fn load_stt(config: &BrainConfig) -> Option<stt::WhisperStt> {
    let encoder = config.stt_encoder.as_deref()?;
    let decoder = config.stt_decoder.as_deref()?;

    // Derive vocab path: same directory as encoder
    let vocab = {
        let dir = std::path::Path::new(encoder)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        dir.join("vocab.json").to_string_lossy().into_owned()
    };

    match stt::WhisperStt::load(encoder, decoder, &vocab) {
        Ok(w) => Some(w),
        Err(e) => {
            log::error!("Brain: STT load failed: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// STT correction — streaming into the input box
// ---------------------------------------------------------------------------

/// Run a short LLM correction pass, streaming each token back as an
/// `InputDraft` so the UI shows the sentence being refined in real time.
/// Returns the final corrected string; falls back to `raw` on any error.
fn stream_correction(
    raw: &str,
    infer_cmd_tx: &std::sync::mpsc::SyncSender<InferCmd>,
    infer_res_rx: &std::sync::mpsc::Receiver<InferRes>,
    response_tx: &std::sync::mpsc::Sender<BrainResponse>,
) -> String {
    let prompt = format!(
        "<|im_start|>system\n\
         Fix any errors in this voice transcription. \
         Output ONLY the corrected text — no explanation, no quotes.\
         <|im_end|>\n\
         <|im_start|>user\n{raw}<|im_end|>\n\
         <|im_start|>assistant\n"
    );

    if infer_cmd_tx.send(InferCmd::Run(prompt)).is_err() {
        log::error!("stream_correction: inference channel closed");
        return raw.to_string();
    }

    let mut result = String::new();
    loop {
        match infer_res_rx.recv() {
            Ok(InferRes::Token(piece)) => {
                result.push_str(&piece);
                // Replace input box content with every new token so the user
                // watches the sentence being corrected word by word.
                let draft = result.trim_start_matches('\n').to_string();
                if !draft.is_empty() {
                    let _ = response_tx.send(BrainResponse::InputDraft { text: draft });
                }
            }
            Ok(InferRes::Done) | Err(_) => break,
            _ => {}
        }
    }

    let corrected = result.trim().to_string();
    if corrected.is_empty() { raw.to_string() } else { corrected }
}

/// Returns true if the text contains CJK (Chinese/Japanese/Korean) characters.
/// Kokoro TTS cannot handle these and egui's default font has no CJK glyphs.
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        // CJK Unified Ideographs, Hiragana, Katakana, Hangul, etc.
        matches!(cp, 0x3000..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
    })
}

fn load_tts(config: &BrainConfig) -> Option<tts::KokoroTts> {
    let model = config.tts_model.as_deref()?;
    let voice = config.tts_voice.as_deref()?;
    let tokenizer = config.tts_tokenizer.as_deref()?;

    match tts::KokoroTts::load(model, voice, tokenizer) {
        Ok(mut kokoro) => {
            if let Some(lang) = &config.tts_lang {
                kokoro.set_lang(lang);
            }
            Some(kokoro)
        }
        Err(e) => {
            log::error!("Brain: TTS load failed: {e}");
            None
        }
    }
}
