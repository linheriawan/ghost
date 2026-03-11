use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use whisper_rs::{WhisperContext, SamplingStrategy};
use dirs::data_dir;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "id")]
    Indonesian,
    #[serde(rename = "zh")]
    Chinese,
    #[serde(rename = "fr")]
    French,
}

impl Language {
    pub fn to_whisper_language(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Indonesian => "id", 
            Language::Chinese => "zh",
            Language::French => "fr",
        }
    }
}

pub struct WhisperSTT {
    contexts: std::collections::HashMap<Language, WhisperContext>,
}

impl WhisperSTT {
    pub fn new() -> Result<Self> {
        let mut contexts = std::collections::HashMap::new();
        
        // Initialize contexts for each language
        for lang in [Language::English, Language::Indonesian, Language::Chinese, Language::French] {
            let model_path = get_model_path(&lang)?;
            let ctx = WhisperContext::new(&model_path.to_string_lossy())?;
            contexts.insert(lang, ctx);
        }
        
        Ok(Self { contexts })
    }
    
    pub fn transcribe(&self, audio_data: &[i16], language: Language) -> Result<String> {
        let ctx = self.contexts.get(&language)
            .ok_or_else(|| anyhow::anyhow!("No model loaded for language: {:?}", language))?;
            
        let mut state = ctx.create_state()?;
        let params = whisper_rs::FullParams::new(SamplingStrategy::default());
        let mut params = params;
        params.set_language(Some(language.to_whisper_language()));
        
        // Convert i16 samples to f32 for Whisper
        let audio_data_f32: Vec<f32> = audio_data.iter()
            .map(|&sample| sample as f32 / 32768.0)  // Normalize i16 to f32 range [-1.0, 1.0]
            .collect();
        
        state.full(params, &audio_data_f32)?;
        
        let mut text = String::new();
        let num_segments = state.full_n_segments()?;
        for i in 0..num_segments {
            let segment = state.full_get_segment_text(i)?;
            text.push_str(&segment);
        }
        
        Ok(text.trim().to_string())
    }
}

fn get_model_path(language: &Language) -> Result<PathBuf> {
    // Determine the appropriate data directory for the OS
    let mut model_dir = data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap())
        .join("rosecli_models");
    
    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&model_dir)?;
    
    // Construct the model filename based on language
    let model_filename = format!("ggml-whisper-{}.bin", match language {
        Language::English => "base.en",
        Language::Indonesian => "base.id", 
        Language::Chinese => "medium.zh",
        Language::French => "base.fr",
    });
    
    Ok(model_dir.join(model_filename))
}