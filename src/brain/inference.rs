//! Local LLM inference via llama-cpp-2 (GGUF models).
//!
//! Direct port of the proven systagent prototype inference module.
//! One `InferenceSession` is loaded once per app lifetime and reused for all queries.

use std::path::Path;

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
};

const MAX_TOKENS: usize = 40_960;

pub struct InferenceSession {
    backend: LlamaBackend,
    model:   LlamaModel,
}

impl InferenceSession {
    /// Load a GGUF model from `model_path`.
    /// `n_gpu_layers = 0` → CPU only; higher values offload layers to Metal/CUDA.
    pub fn load(model_path: &Path, n_gpu_layers: u32) -> anyhow::Result<Self> {
        if !model_path.exists() {
            anyhow::bail!("model file not found: {:?}", model_path);
        }
        let path = model_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 model path"))?;

        log::info!("inference: loading {path}");

        let backend = LlamaBackend::init()
            .map_err(|e| anyhow::anyhow!("backend init: {e}"))?;

        let model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        let model = LlamaModel::load_from_file(&backend, path, &model_params)
            .map_err(|e| anyhow::anyhow!("model load: {e}"))?;

        log::info!("inference: loaded");
        Ok(Self { backend, model })
    }

    /// Run inference, streaming each generated text piece via `on_token`.
    ///
    /// Blocks the calling thread — call from a dedicated OS thread.
    ///
    /// Behaviour:
    /// - Single-token prefill batching (avoids Q8 quantization artifacts on large prompts).
    /// - `<think>` … `</think>` blocks are suppressed (Qwen3 reasoning tokens).
    /// - Stateful sampling chain: penalties → top_k → top_p → temp → dist.
    pub fn run_streaming<F>(&self, prompt: &str, mut on_token: F) -> anyhow::Result<()>
    where
        F: FnMut(String),
    {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(40_960));
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("context: {e}"))?;

        log::debug!("inference: prompt preview: {:?}", &prompt[..prompt.len().min(200)]);

        let tokens = self
            .model
            .str_to_token(prompt, AddBos::Never)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;

        log::info!("inference: prompt tokens = {}", tokens.len());

        if tokens.is_empty() {
            return Ok(());
        }

        // Single-token prefill: avoids Q8 quantization precision artifacts
        // that occur with batch prefill for prompts > ~30 tokens on this model.
        let mut batch = LlamaBatch::new(1, 1);
        for (i, &token) in tokens.iter().enumerate() {
            batch.clear();
            batch
                .add(token, i as i32, &[0], i == tokens.len() - 1)
                .map_err(|e| anyhow::anyhow!("batch add prefill {i}: {e}"))?;
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("prefill decode {i}: {e}"))?;
        }

        log::info!("inference: prefill done, generating");

        let mut n_cur          = 0;
        let mut last_token_pos = tokens.len() as i32;
        let mut decoder        = encoding_rs::UTF_8.new_decoder();
        let mut in_think       = false;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(u32::MAX),
        ]);

        while n_cur < MAX_TOKENS {
            let token_id = sampler.sample(&ctx, batch.n_tokens() - 1);

            if self.model.is_eog_token(token_id) {
                log::debug!("inference: EOG at step {n_cur}");
                break;
            }

            if let Ok(text) = self.model.token_to_piece(token_id, &mut decoder, true, None) {
                match text.as_str() {
                    "<think>"  => { in_think = true; }
                    "</think>" => { in_think = false; }
                    _ if !in_think && !text.is_empty() => { on_token(text); }
                    _ => {}
                }
            }

            sampler.accept(token_id);

            batch = LlamaBatch::new(1, 1);
            batch
                .add(token_id, last_token_pos, &[0.into()], true)
                .map_err(|e| anyhow::anyhow!("batch add step {n_cur}: {e}"))?;
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("decode step {n_cur}: {e}"))?;

            n_cur += 1;
            last_token_pos += 1;
        }

        log::info!("inference: done — {n_cur} tokens generated");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

/// Build a ChatML prompt (Qwen2/Qwen3 format) from history + a new user message.
pub fn build_prompt(system: &str, history: &[(String, String)], user_text: &str) -> String {
    let mut p = format!("<|im_start|>system\n{system}<|im_end|>\n");
    for (user, assistant) in history {
        p.push_str(&format!(
            "<|im_start|>user\n{user}<|im_end|>\n\
             <|im_start|>assistant\n{assistant}<|im_end|>\n"
        ));
    }
    p.push_str(&format!(
        "<|im_start|>user\n{user_text}<|im_end|>\n\
         <|im_start|>assistant\n"
    ));
    p
}
