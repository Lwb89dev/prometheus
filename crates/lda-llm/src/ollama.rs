use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::join_all;
use futures_util::StreamExt;
use lda_common::{join_endpoint, rank_installed, InstalledModel, Message, Result, Role};
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
        let models = models_from_tags(&body)
            .into_iter()
            .map(|model| model.name)
            .collect();
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

impl OllamaProvider {
    /// Read `/api/tags`, then `/api/show` for each name, and rank the result.
    /// At most 24 models are inspected. A failed show leaves `tools` unknown.
    pub async fn describe_models(&self) -> Result<(Vec<InstalledModel>, bool)> {
        let url = join_endpoint(&self.endpoint, "api/tags")?;
        let response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(provider_err)?;
        if !response.status().is_success() {
            return Err(provider_err(format!("ollama tags: {}", response.status())));
        }
        let body: Value = response.json().await.map_err(provider_err)?;
        let mut models = models_from_tags(&body);
        let truncated = models.len() > 24;
        models.truncate(24);
        let shows = join_all(models.iter().map(|model| self.show(&model.name))).await;
        for (model, show) in models.iter_mut().zip(shows) {
            if let Ok(body) = show {
                apply_show(model, &body);
            }
        }
        rank_installed(&mut models);
        Ok((models, truncated))
    }

    pub async fn show(&self, name: &str) -> Result<Value> {
        let url = join_endpoint(&self.endpoint, "api/show")?;
        let response = self
            .http
            .post(url)
            .timeout(Duration::from_secs(10))
            .json(&json!({ "name": name }))
            .send()
            .await
            .map_err(provider_err)?;
        if !response.status().is_success() {
            return Err(provider_err(format!("ollama show: {}", response.status())));
        }
        response.json().await.map_err(provider_err)
    }
}

const PULL_CAP: usize = 24;

/// Stream `/api/pull` until Ollama reports success. `on_status` receives the
/// latest status text. The caller decides whether this download was approved.
pub async fn pull_model(
    http: &reqwest::Client,
    endpoint: &str,
    name: &str,
    mut on_status: impl FnMut(&str),
) -> Result<()> {
    let url = join_endpoint(endpoint, "api/pull")?;
    let response = http
        .post(url)
        .timeout(Duration::from_secs(60 * 60))
        .json(&json!({ "name": name, "stream": true }))
        .send()
        .await
        .map_err(provider_err)?;
    if !response.status().is_success() {
        return Err(provider_err(format!("ollama pull: {}", response.status())));
    }
    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(provider_err)?;
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = pending.find('\n') {
            let line = pending[..index].trim().to_string();
            pending.replace_range(..=index, "");
            if line.is_empty() {
                continue;
            }
            match pull_line(&line) {
                PullLine::Status(text) => on_status(&text),
                PullLine::Done => return Ok(()),
                PullLine::Failed(text) => return Err(provider_err(text)),
            }
        }
    }
    Err(provider_err("ollama pull ended before success".to_string()))
}

#[derive(Debug)]
enum PullLine {
    Status(String),
    Done,
    Failed(String),
}

fn pull_line(line: &str) -> PullLine {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return PullLine::Status("waiting".to_string());
    };
    if let Some(err) = value.get("error").and_then(Value::as_str) {
        return PullLine::Failed(clip_status(err));
    }
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("waiting");
    if status == "success" {
        return PullLine::Done;
    }
    PullLine::Status(clip_status(&pull_progress(status, &value)))
}

fn pull_progress(status: &str, value: &Value) -> String {
    let completed = value.get("completed").and_then(Value::as_u64);
    let total = value.get("total").and_then(Value::as_u64);
    let Some((done, total)) = completed.zip(total) else {
        return status.to_string();
    };
    if total == 0 {
        return status.to_string();
    }
    format!("{status} {}%", done.saturating_mul(100) / total)
}

fn clip_status(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(PULL_CAP * 4) {
        if ch.is_control() {
            continue;
        }
        out.push(ch);
    }
    out
}

pub fn models_from_tags(body: &Value) -> Vec<InstalledModel> {
    let Some(items) = body.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    items.iter().filter_map(model_from_tag).collect()
}

fn model_from_tag(item: &Value) -> Option<InstalledModel> {
    let name = item.get("name").and_then(Value::as_str)?.trim();
    if !selectable_name(name) {
        return None;
    }
    let details = item.get("details");
    Some(InstalledModel {
        name: name.to_string(),
        size_bytes: item.get("size").and_then(Value::as_u64).unwrap_or(0),
        family: text_field(details, "family"),
        parameter_size: text_field(details, "parameter_size"),
        tools: None,
        capabilities: Vec::new(),
    })
}

pub fn apply_show(model: &mut InstalledModel, body: &Value) {
    let Some(caps) = body.get("capabilities").and_then(Value::as_array) else {
        return;
    };
    model.capabilities = caps
        .iter()
        .filter_map(|item| item.as_str())
        .filter(|item| !item.is_empty() && item.len() <= 64 && !item.chars().any(char::is_control))
        .map(str::to_string)
        .collect();
    model.tools = Some(model.capabilities.iter().any(|item| item == "tools"));
}

fn text_field(details: Option<&Value>, key: &str) -> String {
    details
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn selectable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.chars().any(|ch| ch.is_control() || ch.is_whitespace())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_show_keep_unknown_tools_until_capabilities_exist() {
        let body = json!({
            "models": [
                {
                    "name": "qwen2.5:7b",
                    "size": 10,
                    "details": { "family": "qwen2", "parameter_size": "7.6B" }
                },
                { "name": "bad name" },
                { "name": "llama3.1:8b", "size": 4 }
            ]
        });
        let mut models = models_from_tags(&body);
        assert_eq!(models.len(), 2);
        assert!(models[0].tools.is_none());
        apply_show(
            &mut models[0],
            &json!({ "capabilities": ["completion", "tools"] }),
        );
        apply_show(&mut models[1], &json!({ "capabilities": ["completion"] }));
        assert_eq!(models[0].tools, Some(true));
        assert_eq!(models[1].tools, Some(false));
        assert_eq!(
            models[0].capabilities,
            vec!["completion".to_string(), "tools".to_string()]
        );
        assert_eq!(models[0].family, "qwen2");
    }

    #[test]
    fn pull_line_reads_success_error_and_percent() {
        assert!(matches!(
            pull_line(r#"{"status":"success"}"#),
            PullLine::Done
        ));
        assert!(matches!(
            pull_line(r#"{"error":"no such model"}"#),
            PullLine::Failed(_)
        ));
        match pull_line(r#"{"status":"downloading","completed":50,"total":100}"#) {
            PullLine::Status(text) => assert_eq!(text, "downloading 50%"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
