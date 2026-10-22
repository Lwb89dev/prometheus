use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lda_common::{Message, ProviderConfig, ProviderKind, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::anthropic::AnthropicProvider;
use crate::delta::LlmDelta;
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tools: bool,
    pub temperature: bool,
    pub usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub timeout: Duration,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn list_models(&self) -> Result<Vec<String>>;

    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<mpsc::Receiver<Result<LlmDelta>>>;
}

pub fn build_provider(
    config: &ProviderConfig,
    http: reqwest::Client,
    api_key: Option<String>,
) -> Arc<dyn LlmProvider> {
    match config.kind {
        ProviderKind::Ollama => Arc::new(OllamaProvider {
            id: config.id.clone(),
            endpoint: config.endpoint.clone(),
            http,
        }),
        ProviderKind::OpenaiCompatible | ProviderKind::Xai => Arc::new(OpenAiProvider {
            id: config.id.clone(),
            endpoint: config.endpoint.clone(),
            http,
            api_key,
        }),
        ProviderKind::Anthropic => Arc::new(AnthropicProvider {
            id: config.id.clone(),
            endpoint: config.endpoint.clone(),
            http,
            api_key,
        }),
    }
}
