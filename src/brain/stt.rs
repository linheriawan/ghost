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

use ndarray::{Array1, Array2, Array3, Array4, Axis, Ix3, Ix4};
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

// Special token IDs (whisper-base.en)
const TOK_EOT: i64 = 50256;          // <|endoftext|>
const TOK_SOT: i64 = 50258;          // <|startoftranscript|>
const TOK_EN: i64 = 50259;           // <|en|>
const TOK_TRANSCRIBE: i64 = 50360;   // <|transcribe|>
const TOK_NO_TIMESTAMPS: i64 = 50363; // <|notimestamps|>

const MAX_NEW_TOKENS: usize = 448;

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

        let id_to_token = load_vocab(Path::new(vocab_path))?;
        log::info!("WhisperStt: vocab size = {}", id_to_token.len());

        let byte_decoder = build_byte_decoder();
        let mel_fb = build_mel_filterbank(N_MELS, N_FFT / 2 + 1, SAMPLE_RATE);

        Ok(Self { encoder, decoder, id_to_token, byte_decoder, mel_fb })
    }

    /// Transcribe 16 kHz mono PCM → UTF-8 text.
    pub fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }

        // 1. Log-mel spectrogram [1, 80, 3000]
        let mel = self.log_mel_spectrogram(samples);

        // 2. Encoder: audio features → [1, 1500, 512]
        let enc_hidden = self.run_encoder(&mel)?;
        log::debug!("WhisperStt: encoder hidden {:?}", enc_hidden.shape());

        // 3. Greedy decode → token IDs
        let token_ids = self.greedy_decode(&enc_hidden)?;
        log::info!("WhisperStt: {} tokens generated", token_ids.len());

        // 4. Detokenize → UTF-8
        let text = self.detokenize(&token_ids).trim().to_string();
        log::info!("WhisperStt: {:?}", text);
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
// Decoder with k/v cache
// ---------------------------------------------------------------------------

struct LayerCache {
    dec_key: Array4<f32>, // [1, 8, past_dec, 64]
    dec_val: Array4<f32>,
    enc_key: Array4<f32>, // [1, 8, enc_len, 64] — filled after first step
    enc_val: Array4<f32>,
}

impl LayerCache {
    fn empty() -> Self {
        Self {
            dec_key: Array4::zeros((1, N_HEADS, 0, HEAD_DIM)),
            dec_val: Array4::zeros((1, N_HEADS, 0, HEAD_DIM)),
            enc_key: Array4::zeros((1, N_HEADS, 0, HEAD_DIM)),
            enc_val: Array4::zeros((1, N_HEADS, 0, HEAD_DIM)),
        }
    }
}

impl WhisperStt {
    fn greedy_decode(&self, enc_hidden: &Array3<f32>) -> anyhow::Result<Vec<i64>> {
        let init_tokens = vec![TOK_SOT, TOK_EN, TOK_TRANSCRIBE, TOK_NO_TIMESTAMPS];
        let mut cache: Vec<LayerCache> = (0..N_LAYERS).map(|_| LayerCache::empty()).collect();
        let mut generated: Vec<i64> = Vec::new();

        // --- Prefill (use_cache = false) ---
        let input_ids = Array2::from_shape_vec((1, init_tokens.len()), init_tokens)?;
        let (logits, new_cache) = self.decoder_step(&input_ids, enc_hidden, &cache, false)?;
        cache = new_cache;

        // First generated token = argmax of last prefill position
        let seq_len = logits.shape()[1];
        let first = argmax3(&logits, 0, seq_len - 1);
        if first == TOK_EOT || first >= TOK_SOT {
            return Ok(generated);
        }
        generated.push(first);

        // --- Autoregressive generation (use_cache = true) ---
        for _ in 0..MAX_NEW_TOKENS {
            let last = *generated.last().unwrap();
            let input_ids = Array2::from_shape_vec((1, 1), vec![last])?;
            let (logits, new_cache) =
                self.decoder_step(&input_ids, enc_hidden, &cache, true)?;
            cache = new_cache;

            let tok = argmax3(&logits, 0, 0);
            if tok == TOK_EOT || tok >= TOK_SOT {
                break;
            }
            generated.push(tok);
        }

        Ok(generated)
    }

    fn decoder_step(
        &self,
        input_ids: &Array2<i64>,
        enc_hidden: &Array3<f32>,
        cache: &[LayerCache],
        use_cache: bool,
    ) -> anyhow::Result<(Array3<f32>, Vec<LayerCache>)> {
        // Build all arrays before the inputs vec so they live long enough.
        let use_cache_arr = Array1::from_elem((1,), use_cache);

        // Build the decoder input map dynamically.
        let mut inputs: Vec<(Cow<str>, DynValue)> = Vec::new();

        inputs.push((
            "input_ids".into(),
            Tensor::<i64>::from_array(input_ids.view())?.into_dyn(),
        ));
        inputs.push((
            "encoder_hidden_states".into(),
            Tensor::<f32>::from_array(enc_hidden.view())?.into_dyn(),
        ));

        for (i, layer) in cache.iter().enumerate() {
            inputs.push((
                format!("past_key_values.{i}.decoder.key").into(),
                Tensor::<f32>::from_array(layer.dec_key.view())?.into_dyn(),
            ));
            inputs.push((
                format!("past_key_values.{i}.decoder.value").into(),
                Tensor::<f32>::from_array(layer.dec_val.view())?.into_dyn(),
            ));
            inputs.push((
                format!("past_key_values.{i}.encoder.key").into(),
                Tensor::<f32>::from_array(layer.enc_key.view())?.into_dyn(),
            ));
            inputs.push((
                format!("past_key_values.{i}.encoder.value").into(),
                Tensor::<f32>::from_array(layer.enc_val.view())?.into_dyn(),
            ));
        }

        inputs.push((
            "use_cache_branch".into(),
            Tensor::<bool>::from_array(use_cache_arr.view())?.into_dyn(),
        ));

        let outputs = self.decoder.run(inputs)?;

        // Extract logits [1, seq, vocab]
        let logits = outputs["logits"]
            .try_extract_tensor::<f32>()?
            .into_dimensionality::<Ix3>()
            .map_err(|e| anyhow::anyhow!("logits reshape: {e}"))?
            .to_owned();

        // Extract present k/v cache
        let mut new_cache: Vec<LayerCache> = Vec::with_capacity(N_LAYERS);
        for i in 0..N_LAYERS {
            let dk = extract4d(&outputs, &format!("present.{i}.decoder.key"))?;
            let dv = extract4d(&outputs, &format!("present.{i}.decoder.value"))?;
            let ek = extract4d(&outputs, &format!("present.{i}.encoder.key"))?;
            let ev = extract4d(&outputs, &format!("present.{i}.encoder.value"))?;
            new_cache.push(LayerCache { dec_key: dk, dec_val: dv, enc_key: ek, enc_val: ev });
        }

        Ok((logits, new_cache))
    }
}

fn extract4d(outputs: &ort::session::SessionOutputs, name: &str) -> anyhow::Result<Array4<f32>> {
    outputs[name]
        .try_extract_tensor::<f32>()?
        .into_dimensionality::<Ix4>()
        .map_err(|e| anyhow::anyhow!("reshape {name}: {e}"))?
        .to_owned()
        .into_dimensionality::<Ix4>()
        .map_err(|e| anyhow::anyhow!("convert {name}: {e}"))
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
            if id >= TOK_SOT {
                continue; // skip all special tokens
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
