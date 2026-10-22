use std::time::Duration;

use futures_util::StreamExt;
use lda_common::{Error, Result};
use lda_security::redact_secrets;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::delta::{push_lines, LlmDelta};

pub struct JsonStream<'a> {
    pub http: &'a Client,
    pub url: &'a str,
    pub body: &'a Value,
    pub bearer: Option<&'a str>,
    pub headers: &'a [(String, String)],
    pub timeout: Duration,
    pub cancel: CancellationToken,
}

pub async fn post_json_stream<F>(
    call: JsonStream<'_>,
    parse: F,
    tx: mpsc::Sender<Result<LlmDelta>>,
) -> Result<()>
where
    F: Fn(&str) -> Result<Vec<LlmDelta>>,
{
    let JsonStream {
        http,
        url,
        body,
        bearer,
        headers,
        timeout,
        cancel,
    } = call;
    let mut request = http.post(url).timeout(timeout).json(body);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        result = request.send() => result.map_err(provider_err)?,
    };
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "HTTP {status}: {}",
            clip(&redact_secrets(&text), 500)
        )));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            item = stream.next() => item,
        };
        let Some(item) = next else {
            if !buffer.trim().is_empty() {
                send_parsed(&parse, buffer.trim(), &tx).await?;
            }
            break;
        };
        let bytes = item.map_err(provider_err)?;
        let chunk = String::from_utf8_lossy(&bytes);
        for line in push_lines(&mut buffer, &chunk) {
            if send_parsed(&parse, &line, &tx).await?.is_none() {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn send_parsed<F>(
    parse: &F,
    line: &str,
    tx: &mpsc::Sender<Result<LlmDelta>>,
) -> Result<Option<()>>
where
    F: Fn(&str) -> Result<Vec<LlmDelta>>,
{
    for delta in parse(line)? {
        if tx.send(Ok(delta)).await.is_err() {
            return Ok(None);
        }
    }
    Ok(Some(()))
}

pub fn provider_err(err: impl ToString) -> Error {
    Error::Provider(clip(&redact_secrets(&err.to_string()), 500))
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}
