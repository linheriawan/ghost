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
    /// STT transcription result — in voice mode, the UI shows this as a user message.
    Transcription { text: String },
    /// TTS synthesis complete — play the samples on the default audio output.
    SpeechReady {
        samples: Vec<f32>,
        sample_rate: u32,
    },
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

        // Optional STT
        let stt = load_stt(&config);
        if stt.is_some() {
            log::info!("Brain: STT ready");
        } else {
            log::info!("Brain: STT not configured or failed to load");
        }

        let mut history: Vec<(String, String)> = Vec::new();
        let mut voice_mode = false;
        let mut mic_capture: Option<audio::AudioCapture> = None;

        // VAD: energy threshold (RMS), and silence duration before utterance ends.
        // Silence duration is tracked in samples (16 kHz), not frames, so it's
        // independent of the device buffer size.
        let mut vad = audio::VadState::new(0.008, 9_600); // 9600 samples = 600 ms at 16 kHz
        let mut chunk_count = 0u32;

        log::info!("Brain: entering command loop");

        while let Ok(cmd) = cmd_rx.recv() {
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
                    if let Some(ref kokoro) = tts {
                        match kokoro.synthesize(&response) {
                            Ok(samples) if !samples.is_empty() => {
                                let _ = response_tx.send(BrainResponse::SpeechReady {
                                    samples,
                                    sample_rate: tts::SAMPLE_RATE,
                                });
                            }
                            Ok(_) => {
                                log::warn!("Brain: TTS produced empty audio");
                            }
                            Err(e) => {
                                log::error!("Brain: TTS error: {e}");
                            }
                        }
                    }
                }

                BrainCommand::SetVoiceMode(on) => {
                    voice_mode = on;

                    if on {
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

                    // Calculate RMS of this chunk for the level indicator.
                    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
                    let bars = (rms * 400.0).clamp(0.0, 10.0) as usize;
                    let level_str: String = "█".repeat(bars) + &"░".repeat(10 - bars);

                    chunk_count = chunk_count.wrapping_add(1);

                    match vad.feed(&chunk) {
                        VadEvent::Silence => {
                            // Send level update every ~10 chunks to avoid flooding
                            if chunk_count % 10 == 0 {
                                let _ = response_tx.send(BrainResponse::VoiceStatus(
                                    format!("Listening  {level_str}"),
                                ));
                            }
                        }
                        VadEvent::SpeechStart => {
                            let _ = response_tx.send(BrainResponse::VoiceStatus(
                                format!("Hearing you  {level_str}"),
                            ));
                        }
                        VadEvent::Speaking => {
                            // Update level while speaking
                            if chunk_count % 5 == 0 {
                                let _ = response_tx.send(BrainResponse::VoiceStatus(
                                    format!("Hearing you  {level_str}"),
                                ));
                            }
                        }
                        VadEvent::UtteranceEnd(utterance) => {
                            log::info!("Brain: utterance end ({} samples)", utterance.len());
                            let _ = response_tx.send(BrainResponse::VoiceStatus(
                                "Transcribing...".to_string(),
                            ));

                            if let Some(ref s) = stt {
                                match s.transcribe(&utterance) {
                                    Ok(text) if !text.trim().is_empty() => {
                                        log::info!("Brain: transcription = {:?}", text);
                                        let _ = response_tx
                                            .send(BrainResponse::Transcription { text });
                                    }
                                    Ok(_) => {
                                        log::debug!("Brain: empty transcription");
                                        let _ = response_tx.send(BrainResponse::VoiceStatus(
                                            "Listening  ░░░░░░░░░░".to_string(),
                                        ));
                                    }
                                    Err(e) => {
                                        log::error!("Brain: STT error: {e}");
                                        let _ = response_tx.send(BrainResponse::VoiceStatus(
                                            format!("STT error: {e}"),
                                        ));
                                    }
                                }
                            } else {
                                log::warn!("Brain: STT not configured");
                                let _ = response_tx.send(BrainResponse::VoiceStatus(
                                    "STT not configured".to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Signal the inference thread to exit.
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
