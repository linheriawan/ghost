use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub host: String,
    pub port: u16,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 11434,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user", "assistant", or "system"
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: Option<bool>,
    pub options: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub model: String,
    pub created_at: String,
    pub message: ChatMessage,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub modified_at: String,
    pub size: u64,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListModelsResponse {
    pub models: Vec<ModelInfo>,
}

pub struct OllamaClient {
    client: reqwest::Client,
    config: OllamaConfig,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("http://{}:{}/api/chat", self.config.host, self.config.port);
        
        let response = self.client
            .post(&url)
            .json(request)
            .send()
            .await?;

        if response.status().is_success() {
            let chat_response: ChatResponse = response.json().await?;
            Ok(chat_response)
        } else {
            let error_text = response.text().await?;
            Err(anyhow::anyhow!("Ollama API error: {}", error_text))
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("http://{}:{}/api/tags", self.config.host, self.config.port);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;

        if response.status().is_success() {
            let list_response: ListModelsResponse = response.json().await?;
            Ok(list_response.models)
        } else {
            let error_text = response.text().await?;
            Err(anyhow::anyhow!("Ollama API error: {}", error_text))
        }
    }

    pub async fn generate(&self, model: &str, prompt: &str) -> Result<String> {
        #[derive(Serialize)]
        struct GenerateRequest {
            model: String,
            prompt: String,
            stream: bool,
        }

        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            stream: false,
        };

        let url = format!("http://{}:{}/api/generate", self.config.host, self.config.port);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct GenerateResponse {
                model: String,
                created_at: String,
                response: String,
                done: bool,
            }

            let gen_response: GenerateResponse = response.json().await?;
            Ok(gen_response.response)
        } else {
            let error_text = response.text().await?;
            Err(anyhow::anyhow!("Ollama API error: {}", error_text))
        }
    }

    pub async fn pull_model(&self, model_name: &str) -> Result<()> {
        #[derive(Serialize)]
        struct PullRequest {
            name: String,
        }

        let request = PullRequest {
            name: model_name.to_string(),
        };

        let url = format!("http://{}:{}/api/pull", self.config.host, self.config.port);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            // For pull operations, we might want to stream the response to show progress
            // For now, just check if the request was successful
            Ok(())
        } else {
            let error_text = response.text().await?;
            Err(anyhow::anyhow!("Ollama API error: {}", error_text))
        }
    }
}

// Convenience function to create a default client
pub fn create_default_client() -> OllamaClient {
    OllamaClient::new(OllamaConfig::default())
}