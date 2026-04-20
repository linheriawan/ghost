//! Kokoro TTS — ONNX-based text-to-speech via ort.
//!
//! Port of the proven test_onnx/kokoro.rs implementation.
//! Uses espeak-ng for phonemization and onnx-community/Kokoro-82M-ONNX for synthesis.
//!
//! Required files:
//!   model.onnx         — from onnx-community/Kokoro-82M-ONNX (HuggingFace)
//!   af_bella.bin       — voice style embeddings (512 rows × 256 floats)
//!   tokenizer.json     — HuggingFace tokenizer format (vocab only)

use std::collections::HashMap;
use std::path::Path;

use ndarray::{Array1, Array2};
use ort::session::Session;

pub const SAMPLE_RATE: u32 = 24_000;

/// Kokoro-82M text-to-speech engine.
pub struct KokoroTts {
    session: Session,
    vocab: HashMap<char, i64>,
    all_styles: Vec<f32>,
    /// espeak-ng language code ("a" = en-us, "b" = en-gb, etc.)
    lang: String,
    speed: f32,
}

impl KokoroTts {
    /// Load the Kokoro ONNX model, voice style file, and tokenizer vocab.
    pub fn load(
        model_path: &str,
        voice_path: &str,
        tokenizer_path: &str,
    ) -> anyhow::Result<Self> {
        for (label, path) in &[
            ("model", model_path),
            ("voice", voice_path),
            ("tokenizer", tokenizer_path),
        ] {
            if !Path::new(path).exists() {
                anyhow::bail!("TTS {label} not found: {path}");
            }
        }

        log::info!("KokoroTts: loading model from {model_path}");
        let session = Session::builder()?.commit_from_file(model_path)?;

        let vocab = load_vocab(Path::new(tokenizer_path))?;
        log::info!("KokoroTts: vocab size = {}", vocab.len());

        let all_styles = load_style(Path::new(voice_path))?;
        log::info!("KokoroTts: style rows = {}", all_styles.len() / 256);

        Ok(Self {
            session,
            vocab,
            all_styles,
            lang: "a".to_string(), // en-us default
            speed: 1.0,
        })
    }

    /// Set the espeak-ng language code (default "a" = en-us).
    pub fn set_lang(&mut self, lang: &str) {
        self.lang = lang.to_string();
    }

    /// Set speech speed multiplier (default 1.0).
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// Synthesize `text` and return raw 24 kHz mono f32 PCM samples.
    pub fn synthesize(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        let espeak_voice = lang_to_espeak(&self.lang);

        // 1. Phonemize via espeak-ng (or fall back to espeak)
        let ipa = phonemize(text, espeak_voice);

        log::debug!("KokoroTts: phonemes: {ipa}");

        // 2. Tokenize: map IPA chars → token IDs (skip unknowns)
        let ids: Vec<i64> = ipa.chars().filter_map(|c| self.vocab.get(&c).copied()).collect();
        if ids.is_empty() {
            anyhow::bail!("KokoroTts: no known phoneme tokens — check espeak-ng is installed");
        }

        // 3. Build tensors
        let seq_len = ids.len();
        let input_ids = Array2::from_shape_vec((1, seq_len), ids)
            .map_err(|e| anyhow::anyhow!("input_ids shape error: {e}"))?;
        let style_row = get_style_row(&self.all_styles, seq_len);
        let style = Array2::from_shape_vec((1, 256), style_row)
            .map_err(|e| anyhow::anyhow!("style shape error: {e}"))?;
        let speed_arr = Array1::from_vec(vec![self.speed]);

        // 4. Run ONNX inference
        let outputs = self.session.run(ort::inputs! {
            "input_ids" => input_ids.view(),
            "style"     => style.view(),
            "speed"     => speed_arr.view(),
        }?)?;

        // 5. Extract waveform [1, num_samples]
        let waveform = outputs["waveform"].try_extract_tensor::<f32>()?;
        let samples: Vec<f32> = waveform.view().iter().copied().collect();

        log::info!(
            "KokoroTts: synthesized {} samples ({:.1}s)",
            samples.len(),
            samples.len() as f32 / SAMPLE_RATE as f32
        );

        Ok(samples)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_vocab(path: &Path) -> anyhow::Result<HashMap<char, i64>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read tokenizer: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("tokenizer JSON: {e}"))?;

    let vocab_obj = json["model"]["vocab"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("tokenizer missing model.vocab"))?;

    let map = vocab_obj
        .iter()
        .filter_map(|(k, v)| {
            let ch = k.chars().next()?;
            Some((ch, v.as_i64()?))
        })
        .collect();

    Ok(map)
}

fn load_style(path: &Path) -> anyhow::Result<Vec<f32>> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("cannot read voice style: {e}"))?;
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    Ok(floats)
}

fn get_style_row(all_styles: &[f32], phoneme_len: usize) -> Vec<f32> {
    let idx = phoneme_len % 512;
    all_styles[idx * 256..(idx + 1) * 256].to_vec()
}

fn phonemize(text: &str, voice: &str) -> String {
    // Try espeak-ng first (preferred), fall back to espeak
    for cmd in &["espeak-ng", "espeak"] {
        if let Some(ipa) = std::process::Command::new(cmd)
            .args(["-q", "--ipa", "-v", voice, text])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
        {
            log::debug!("KokoroTts: phonemized via {cmd}: {ipa}");
            return ipa;
        }
    }
    log::warn!("KokoroTts: espeak/espeak-ng not found — using raw text (quality will be poor)");
    text.to_string()
}

fn lang_to_espeak(lang: &str) -> &str {
    match lang {
        "a" => "en-us",
        "b" => "en-gb",
        "e" => "es",
        "f" => "fr-fr",
        "h" => "hi",
        "i" => "it",
        "p" => "pt-br",
        "de" => "de",
        "id" => "id",
        "ko" => "ko",
        "ru" => "ru",
        _ => "en-us",
    }
}
