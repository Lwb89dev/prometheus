use std::time::Duration;

use async_trait::async_trait;
use lda_common::{join_endpoint, Message, Result, Role};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::delta::LlmDelta;
use crate::http::{post_json_stream, provider_err, JsonStream};
use crate::parse::parse_ollama_line;
use crate::provider::{ChatRequest, LlmProvider, ProviderCapabilities, ToolDefinition};

pub struct OllamaProvider {
    pub id: String,
    pub endpoint: String,
    pub http: reqwest::Client,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
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
        let url = join_endpoint(&self.endpoint, "api/tags")?;
        let response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(provider_err)?;
        let body: Value = response.json().await.map_err(provider_err)?;
        let models = body
            .get("models")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_string))
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
        let url = join_endpoint(&self.endpoint, "api/chat")?;
        let body = chat_body(&request);
        let http = self.http.clone();
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let result = post_json_stream(
                JsonStream {
                    http: &http,
                    url: &url,
                    body: &body,
                    bearer: None,
                    headers: &[],
                    timeout: request.timeout,
                    cancel,
                },
                parse_ollama_line,
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
    let messages: Vec<Value> = request.messages.iter().map(ollama_message).collect();
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(request.tools.iter().map(ollama_tool).collect());
    }
    if let Some(temperature) = request.temperature {
        body["options"] = json!({ "temperature": temperature });
    }
    body
}

fn ollama_message(message: &Message) -> Value {
    let mut value = json!({
        "role": message.role.as_str(),
        "content": message.content,
    });
    if !message.tool_calls.is_empty() {
        value["tool_calls"] = Value::Array(
            message
                .tool_calls
                .iter()
                .enumerate()
                .map(|(index, call)| {
                    json!({
                        "type": "function",
                        "function": {
                            "index": index,
                            "name": call.name,
                            "arguments": call.arguments,
                        }
                    })
                })
                .collect(),
        );
    }
    if message.role == Role::Tool {
        if let Some(name) = &message.tool_name {
            value["tool_name"] = json!(name);
        }
    }
    value
}

fn ollama_tool(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}
