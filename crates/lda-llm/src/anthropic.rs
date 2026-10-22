//! Anthropic Messages API. It is not the OpenAI chat-completions shape.

use std::time::Duration;

use async_trait::async_trait;
use lda_common::{join_endpoint, Message, Result, Role};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::delta::LlmDelta;
use crate::http::{post_json_stream, provider_err, JsonStream};
use crate::parse::parse_anthropic_line;
use crate::provider::{ChatRequest, LlmProvider, ProviderCapabilities, ToolDefinition};

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    pub id: String,
    pub endpoint: String,
    pub http: reqwest::Client,
    pub api_key: Option<String>,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
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
        let mut request = self
            .http
            .get(url)
            .timeout(Duration::from_secs(15))
            .header("anthropic-version", ANTHROPIC_VERSION);
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
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
        let url = join_endpoint(&self.endpoint, "messages")?;
        let body = chat_body(&request);
        let http = self.http.clone();
        let mut headers = vec![("anthropic-version".into(), ANTHROPIC_VERSION.into())];
        if let Some(key) = &self.api_key {
            headers.push(("x-api-key".into(), key.clone()));
        }
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let result = post_json_stream(
                JsonStream {
                    http: &http,
                    url: &url,
                    body: &body,
                    bearer: None,
                    headers: &headers,
                    timeout: request.timeout,
                    cancel,
                },
                parse_anthropic_line,
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
    let system = request
        .messages
        .iter()
        .filter(|message| message.role == Role::System)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages = anthropic_messages(&request.messages);
    let mut body = json!({
        "model": request.model,
        "max_tokens": 1024,
        "stream": true,
        "messages": messages,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(request.tools.iter().map(anthropic_tool).collect());
    }
    body
}

fn anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        if message.role == Role::System {
            index += 1;
            continue;
        }
        if message.role == Role::Tool {
            let mut blocks = Vec::new();
            while index < messages.len() && messages[index].role == Role::Tool {
                blocks.push(tool_result(&messages[index]));
                index += 1;
            }
            out.push(json!({"role": "user", "content": blocks}));
            continue;
        }
        out.push(one_message(message));
        index += 1;
    }
    out
}

fn one_message(message: &Message) -> Value {
    if message.role == Role::Assistant && !message.tool_calls.is_empty() {
        let mut blocks = Vec::new();
        if !message.content.is_empty() {
            blocks.push(json!({"type": "text", "text": message.content}));
        }
        for call in &message.tool_calls {
            blocks.push(json!({
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                "input": call.arguments,
            }));
        }
        return json!({"role": "assistant", "content": blocks});
    }
    json!({
        "role": message.role.as_str(),
        "content": message.content,
    })
}

fn tool_result(message: &Message) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
        "content": message.content,
    })
}

fn anthropic_tool(tool: &ToolDefinition) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.parameters,
    })
}
