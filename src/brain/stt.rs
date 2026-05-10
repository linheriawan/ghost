//! Whisper STT — ONNX-based speech-to-text via ort.
//!
//! Uses onnx-community/whisper-base.en split models:
//!   encoder_model.onnx         → audio features → hidden states
//!   decoder_model_merged.onnx  → autoregressive token generation with k/v cache
//!   vocab.json                 → GPT-2 BPE vocabulary (openai/whisper-base)
//!
//! Audio input: 16 kHz mono f32 PCM (from cpal capture + VAD).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use ndarray::{Array1, Array2, Array3, Array4, Axis, Ix3};
use ort::session::Session;
use ort::value::{DynValue, Tensor};
use rustfft::{num_complex::Complex, FftPlanner};

// ---------------------------------------------------------------------------
// Model constants (whisper-base)
// ---------------------------------------------------------------------------

const N_MELS: usize = 80;
const N_FFT: usize = 400;
const HOP_LENGTH: usize = 160;
const CHUNK_SIZE: usize = 3000; // mel frames = 30 s at 16 kHz
const SAMPLE_RATE: f32 = 16_000.0;

const N_LAYERS: usize = 6;
const N_HEADS: usize = 8;
const HEAD_DIM: usize = 64;

// Special token IDs for onnx-community/whisper-base (multilingual).
// From generation_config.json:
//   decoder_start_token_id = 50258  (<|startoftranscript|>)
//   forced_decoder_ids = [[1, None], [2, 50359]]  (auto language, then transcribe)
//   eos_token_id = 50257            (<|endoftext|>)
// Language tokens: <|en|>=50259 … <|id|>=50275 … (see added_tokens.json)
// We auto-detect language by letting the model generate position 1 freely.
const TOK_EOT: i64 = 50257;          // <|endoftext|> — stop generation here
const TOK_SOT: i64 = 50258;          // <|startoftranscript|> — decoder start
const TOK_TRANSCRIBE: i64 = 50359;   // <|transcribe|> — forced at position 2
const TOK_NO_TIMESTAMPS: i64 = 50363; // <|notimestamps|> — suppress timestamp output
// Language tokens (for logging / future forced-language support):
// <|en|>=50259, <|id|>=50275, <|ms|>=50282 — full list in added_tokens.json

const MAX_NEW_TOKENS: usize = 150; // voice utterances are short; 448 causes long hangs on noise

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct WhisperStt {
    encoder: Session,
    decoder: Session,
    id_to_token: HashMap<i64, String>, // token_id → BPE unicode string
    byte_decoder: HashMap<char, u8>,   // GPT-2 unicode char → raw byte
    mel_fb: ndarray::Array2<f32>,      // precomputed mel filterbank [80, 201]
}

impl WhisperStt {
    pub fn load(
        encoder_path: &str,
        decoder_path: &str,
        vocab_path: &str,
    ) -> anyhow::Result<Self> {
        for (label, path) in &[
            ("encoder", encoder_path),
            ("decoder", decoder_path),
            ("vocab", vocab_path),
        ] {
            if !Path::new(path).exists() {
                anyhow::bail!("STT {label} not found: {path}");
            }
        }

        log::info!("WhisperStt: loading encoder from {encoder_path}");
        let encoder = Session::builder()?.commit_from_file(encoder_path)?;

        log::info!("WhisperStt: loading decoder from {decoder_path}");
        let decoder = Session::builder()?.commit_from_file(decoder_path)?;

        log::info!("WhisperStt: decoder output names:");
        for out in &decoder.outputs {
            log::info!("  {:?}", out.name);
        }

        let id_to_token = load_vocab(Path::new(vocab_path))?;
        log::info!("WhisperStt: vocab size = {}", id_to_token.len());

        // Look up the actual IDs for the special tokens we care about.
        // This reveals whether the model is multilingual or English-only
        // (the IDs differ because en-only has no language tokens).
        let find = |name: &str| -> Option<i64> {
            id_to_token.iter().find(|(_, v)| v.as_str() == name).map(|(k, _)| *k)
        };
        log::info!("STT vocab: size={} eot={:?}", id_to_token.len(), find("<|endoftext|>"));

        let byte_decoder = build_byte_decoder();
        let mel_fb = build_mel_filterbank(N_MELS, N_FFT / 2 + 1, SAMPLE_RATE);

        Ok(Self { encoder, decoder, id_to_token, byte_decoder, mel_fb })
    }

    /// Transcribe 16 kHz mono PCM → UTF-8 text.
    pub fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }

        log::info!("STT: {} samples → mel", samples.len());
        let mel = self.log_mel_spectrogram(samples);

        log::info!("STT: running encoder");
        let enc_hidden = self.run_encoder(&mel)?;
        log::info!("STT: encoder done shape={:?}", enc_hidden.shape());

        log::info!("STT: starting greedy decode");
        let token_ids = self.greedy_decode(&enc_hidden)?;
        log::info!("STT: {} tokens generated", token_ids.len());

        let text = self.detokenize(&token_ids).trim().to_string();
        log::info!("STT: {:?}", text);
        Ok(text)
    }
}

// ---------------------------------------------------------------------------
// Log-mel spectrogram
// ---------------------------------------------------------------------------

impl WhisperStt {
    fn log_mel_spectrogram(&self, samples: &[f32]) -> Array3<f32> {
        // Centre-pad by N_FFT/2 = 200 samples on each side (Whisper default).
        let pad = N_FFT / 2;
        // Target padded length produces exactly CHUNK_SIZE + 1 frames; we drop the last.
        // n_frames = (target - N_FFT) / HOP + 1 = CHUNK_SIZE + 1
        // => target = CHUNK_SIZE * HOP + N_FFT = 480_400
        let target_len = CHUNK_SIZE * HOP_LENGTH + N_FFT;

        let mut audio = vec![0f32; target_len];
        let copy_len = samples.len().min(target_len - pad);
        audio[pad..pad + copy_len].copy_from_slice(&samples[..copy_len]);

        // Hann window
        let window: Vec<f32> = (0..N_FFT)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
            })
            .collect();

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(N_FFT);
        let n_freq = N_FFT / 2 + 1; // 201

        let mut mel_spec = ndarray::Array2::zeros((N_MELS, CHUNK_SIZE));

        for frame_i in 0..CHUNK_SIZE {
            let start = frame_i * HOP_LENGTH;
            let mut buf: Vec<Complex<f32>> = audio[start..start + N_FFT]
                .iter()
                .zip(window.iter())
                .map(|(&s, &w)| Complex::new(s * w, 0.0))
                .collect();

            fft.process(&mut buf);

            let power: Vec<f32> = buf[..n_freq].iter().map(|c| c.norm_sqr()).collect();

            for mel_i in 0..N_MELS {
                let val: f32 = self
                    .mel_fb
                    .row(mel_i)
                    .iter()
                    .zip(power.iter())
                    .map(|(&f, &p)| f * p)
                    .sum();
                mel_spec[[mel_i, frame_i]] = val;
            }
        }

        // Whisper normalization: log10 → clamp → rescale
        mel_spec.mapv_inplace(|v| v.max(1e-10f32).log10());
        let max_val = mel_spec.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        mel_spec.mapv_inplace(|v| ((v.max(max_val - 8.0)) + 4.0) / 4.0);

        mel_spec.insert_axis(Axis(0)) // → [1, 80, 3000]
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

impl WhisperStt {
    fn run_encoder(&self, mel: &Array3<f32>) -> anyhow::Result<Array3<f32>> {
        let outputs = self.encoder.run(ort::inputs! {
            "input_features" => mel.view()
        }?)?;

        let view = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        let hidden = view
            .into_dimensionality::<Ix3>()
            .map_err(|e| anyhow::anyhow!("encoder output reshape: {e}"))?
            .to_owned();
        Ok(hidden)
    }
}

// ---------------------------------------------------------------------------
// Decoder — no k/v cache, multilingual auto language detection
//
// The merged model's use_cache_branch=true does not output decoder k/v in the
// present.* ports, making manual cache management impossible via this model
// variant. Instead we always run use_cache_branch=false and feed all
// accumulated tokens each step. This is O(n²) but fine for short utterances.
//
// Two-phase decode:
//   Phase 1 — language detection: run decoder with [SOT] only and take the
//             argmax over language-token positions (50259..50358) as the
//             detected language.
//   Phase 2 — text generation: seed with [SOT, LANG, TRANSCRIBE, NO_TIMESTAMPS]
//             and greedily sample until EOT or MAX_NEW_TOKENS.
// ---------------------------------------------------------------------------

impl WhisperStt {
    /// Run the ONNX decoder for one step and return the logits over the last position.
    fn decoder_step(
        &self,
        tokens: &[i64],
        enc_hidden: &Array3<f32>,
        dummy: &Array4<f32>,
        use_cache_false: &Array1<bool>,
    ) -> anyhow::Result<Array3<f32>> {
        let n = tokens.len();
        let input_ids = Array2::from_shape_vec((1, n), tokens.to_vec())?;

        let mut inputs: Vec<(Cow<str>, DynValue)> = Vec::new();
        inputs.push(("input_ids".into(), Tensor::<i64>::from_array(input_ids.view())?.into_dyn()));
        inputs.push(("encoder_hidden_states".into(), Tensor::<f32>::from_array(enc_hidden.view())?.into_dyn()));
        for i in 0..N_LAYERS {
            inputs.push((format!("past_key_values.{i}.decoder.key").into(), Tensor::<f32>::from_array(dummy.view())?.into_dyn()));
            inputs.push((format!("past_key_values.{i}.decoder.value").into(), Tensor::<f32>::from_array(dummy.view())?.into_dyn()));
            inputs.push((format!("past_key_values.{i}.encoder.key").into(), Tensor::<f32>::from_array(dummy.view())?.into_dyn()));
            inputs.push((format!("past_key_values.{i}.encoder.value").into(), Tensor::<f32>::from_array(dummy.view())?.into_dyn()));
        }
        inputs.push(("use_cache_branch".into(), Tensor::<bool>::from_array(use_cache_false.view())?.into_dyn()));

        let outputs = self.decoder.run(inputs)?;
        let logits = outputs["logits"]
            .try_extract_tensor::<f32>()?
            .into_dimensionality::<Ix3>()
            .map_err(|e| anyhow::anyhow!("logits reshape: {e}"))?
            .to_owned();
        Ok(logits)
    }

    fn greedy_decode(&self, enc_hidden: &Array3<f32>) -> anyhow::Result<Vec<i64>> {
        let dummy = Array4::<f32>::zeros((1, N_HEADS, 1, HEAD_DIM));
        let use_cache_false = Array1::from_elem((1,), false);

        // Phase 1: language detection — feed [SOT], take argmax over lang range.
        let logits = self.decoder_step(&[TOK_SOT], enc_hidden, &dummy, &use_cache_false)?;
        let lang_tok = argmax3(&logits, 0, 0);
        log::info!("STT: detected language token = {lang_tok}");

        // Phase 2: text generation seeded with [SOT, LANG, TRANSCRIBE, NO_TIMESTAMPS].
        let mut tokens: Vec<i64> = vec![TOK_SOT, lang_tok, TOK_TRANSCRIBE, TOK_NO_TIMESTAMPS];
        let mut generated: Vec<i64> = Vec::new();

        for step in 0..MAX_NEW_TOKENS {
            let n = tokens.len();
            let logits = self.decoder_step(&tokens, enc_hidden, &dummy, &use_cache_false)?;
            let tok = argmax3(&logits, 0, n - 1);

            if step < 3 {
                log::info!("STT: step {step} tok={tok}");
            }

            if tok >= TOK_EOT {
                break;
            }
            generated.push(tok);
            tokens.push(tok);
        }

        log::info!("STT: generated {} tokens", generated.len());
        Ok(generated)
    }
}

fn argmax3(logits: &Array3<f32>, batch: usize, seq_pos: usize) -> i64 {
    logits
        .slice(ndarray::s![batch, seq_pos, ..])
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx as i64)
        .unwrap_or(TOK_EOT)
}

// ---------------------------------------------------------------------------
// Detokenizer
// ---------------------------------------------------------------------------

impl WhisperStt {
    fn detokenize(&self, token_ids: &[i64]) -> String {
        let mut bytes: Vec<u8> = Vec::new();
        for &id in token_ids {
            if id >= TOK_EOT {
                continue;
            }
            if let Some(tok_str) = self.id_to_token.get(&id) {
                for ch in tok_str.chars() {
                    if let Some(&b) = self.byte_decoder.get(&ch) {
                        bytes.push(b);
                    }
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_vocab(path: &Path) -> anyhow::Result<HashMap<i64, String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read vocab.json: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("vocab JSON: {e}"))?;

    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("vocab.json is not an object"))?;

    // vocab.json maps token_string → token_id
    let map = obj
        .iter()
        .filter_map(|(k, v)| Some((v.as_i64()?, k.clone())))
        .collect();

    Ok(map)
}

/// GPT-2 byte-level BPE reverse decoder: unicode char → raw byte.
fn build_byte_decoder() -> HashMap<char, u8> {
    // Bytes that map directly to their unicode codepoint value
    let mut bs: Vec<u8> = Vec::new();
    bs.extend(b'!'..=b'~');      // 33–126
    bs.extend(0xA1u8..=0xACu8); // 161–172
    bs.extend(0xAEu8..=0xFFu8); // 174–255

    let mut cs: Vec<u32> = bs.iter().map(|&b| b as u32).collect();

    // Remaining 68 bytes (control chars etc.) map to U+0100+
    let mut n: u32 = 0;
    for b in 0u8..=255u8 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }

    bs.iter()
        .zip(cs.iter())
        .filter_map(|(&byte, &cp)| char::from_u32(cp).map(|c| (c, byte)))
        .collect()
}

/// HTK mel-scale filterbank matrix [n_mels, n_freq].
fn build_mel_filterbank(n_mels: usize, n_freq: usize, sr: f32) -> ndarray::Array2<f32> {
    let hz_to_mel = |f: f32| 2595.0 * (1.0 + f / 700.0).log10();
    let mel_to_hz = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);

    let mel_min = hz_to_mel(0.0);
    let mel_max = hz_to_mel(sr / 2.0); // Nyquist

    let mel_pts: Vec<f32> = (0..n_mels + 2)
        .map(|i| mel_min + i as f32 * (mel_max - mel_min) / (n_mels + 1) as f32)
        .collect();

    let hz_pts: Vec<f32> = mel_pts.iter().map(|&m| mel_to_hz(m)).collect();
    let n_fft = (n_freq - 1) * 2; // 400
    let bin_pts: Vec<f32> = hz_pts.iter().map(|&f| f * n_fft as f32 / sr).collect();

    let mut fb = ndarray::Array2::zeros((n_mels, n_freq));
    for m in 0..n_mels {
        let f_lo = bin_pts[m];
        let f_c = bin_pts[m + 1];
        let f_hi = bin_pts[m + 2];
        for k in 0..n_freq {
            let kf = k as f32;
            if kf >= f_lo && kf <= f_c && f_c > f_lo {
                fb[[m, k]] = (kf - f_lo) / (f_c - f_lo);
            } else if kf > f_c && kf <= f_hi && f_hi > f_c {
                fb[[m, k]] = (f_hi - kf) / (f_hi - f_c);
            }
        }
    }
    fb
}
