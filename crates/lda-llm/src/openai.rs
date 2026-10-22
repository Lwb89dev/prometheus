use std::time::Duration;

use async_trait::async_trait;
use lda_common::{join_endpoint, Message, Result, Role};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::delta::LlmDelta;
use crate::http::{post_json_stream, provider_err, JsonStream};
use crate::parse::parse_openai_line;
use crate::provider::{ChatRequest, LlmProvider, ProviderCapabilities, ToolDefinition};

pub struct OpenAiProvider {
    pub id: String,
    pub endpoint: String,
    pub http: reqwest::Client,
    pub api_key: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            temperature: true,
            usage: true,
        }
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let url = join_endpoint(&self.endpoint, "models")?;
        let mut request = self.http.get(url).timeout(Duration::from_secs(15));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().await.map_err(provider_err)?;
        if !response.status().is_success() {
            return Err(provider_err(format!("HTTP {}", response.status())));
        }
        let body: Value = response.json().await.map_err(provider_err)?;
        let models = body
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<mpsc::Receiver<Result<LlmDelta>>> {
        let url = join_endpoint(&self.endpoint, "chat/completions")?;
        let body = chat_body(&request);
        let http = self.http.clone();
        let bearer = self.api_key.clone();
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let result = post_json_stream(
                JsonStream {
                    http: &http,
                    url: &url,
                    body: &body,
                    bearer: bearer.as_deref(),
                    headers: &[],
                    timeout: request.timeout,
                    cancel,
                },
                parse_openai_line,
                tx.clone(),
            )
            .await;
            if let Err(err) = result {
                let _ = tx.send(Err(err)).await;
            }
        });
        Ok(rx)
    }
}

fn chat_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(openai_message).collect();
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(request.tools.iter().map(openai_tool).collect());
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    body
}

fn openai_message(message: &Message) -> Value {
    let mut value = json!({
        "role": message.role.as_str(),
        "content": message.content,
    });
    if !message.tool_calls.is_empty() {
        value["tool_calls"] = Value::Array(
            message
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments.to_string(),
                        }
                    })
                })
                .collect(),
        );
    }
    if message.role == Role::Tool {
        if let Some(id) = &message.tool_call_id {
            value["tool_call_id"] = json!(id);
        }
    }
    value
}

fn openai_tool(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}
