use lda_common::{Error, Result};
use serde_json::Value;

use crate::delta::LlmDelta;

pub fn parse_ollama_line(line: &str) -> Result<Vec<LlmDelta>> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|err| Error::Provider(format!("ollama response: {err}")))?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(Error::Provider(error.to_string()));
    }
    let mut out = Vec::new();
    if let Some(message) = value.get("message") {
        push_text(&mut out, message.get("content"));
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for (index, call) in calls.iter().enumerate() {
                let function = call.get("function").unwrap_or(call);
                out.push(tool_delta(
                    index,
                    None,
                    function.get("name"),
                    function.get("arguments"),
                    true,
                ));
            }
        }
    }
    if value.get("done").and_then(Value::as_bool) == Some(true) {
        out.push(LlmDelta::Usage {
            input_tokens: u32_field(&value, "prompt_eval_count"),
            output_tokens: u32_field(&value, "eval_count"),
        });
        out.push(LlmDelta::Finished);
    }
    Ok(out)
}

pub fn parse_openai_line(line: &str) -> Result<Vec<LlmDelta>> {
    let Some(data) = line.trim().strip_prefix("data:") else {
        return Ok(Vec::new());
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(vec![LlmDelta::Finished]);
    }
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(data)
        .map_err(|err| Error::Provider(format!("openai response: {err}")))?;
    if let Some(error) = value.get("error") {
        return Err(Error::Provider(error.to_string()));
    }
    let mut out = Vec::new();
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        out.push(LlmDelta::Usage {
            input_tokens: u32_field(usage, "prompt_tokens"),
            output_tokens: u32_field(usage, "completion_tokens"),
        });
    }
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return Ok(out);
    };
    for choice in choices {
        let Some(delta) = choice.get("delta") else {
            continue;
        };
        push_text(&mut out, delta.get("content"));
        let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for call in calls {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let id = call.get("id").and_then(Value::as_str).map(str::to_string);
            let function = call.get("function");
            let name = function.and_then(|item| item.get("name"));
            let arguments = function.and_then(|item| item.get("arguments"));
            out.push(tool_delta(index, id, name, arguments, false));
        }
    }
    Ok(out)
}

pub fn parse_anthropic_line(line: &str) -> Result<Vec<LlmDelta>> {
    let Some(data) = line.trim().strip_prefix("data:") else {
        return Ok(Vec::new());
    };
    let data = data.trim();
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(data)
        .map_err(|err| Error::Provider(format!("anthropic response: {err}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("anthropic request failed");
        return Err(Error::Provider(message.to_string()));
    }
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();
    match kind {
        "content_block_start" => push_anthropic_start(&mut out, &value),
        "content_block_delta" => push_anthropic_delta(&mut out, &value),
        "message_start" | "message_delta" => push_anthropic_usage(&mut out, &value),
        "message_stop" => out.push(LlmDelta::Finished),
        _ => {}
    }
    Ok(out)
}

fn push_anthropic_start(out: &mut Vec<LlmDelta>, value: &Value) {
    let Some(block) = value.get("content_block") else {
        return;
    };
    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return;
    }
    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    out.push(LlmDelta::ToolCall {
        index,
        id: block.get("id").and_then(Value::as_str).map(str::to_string),
        name: block
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        arguments: None,
        replace_arguments: false,
    });
}

fn push_anthropic_delta(out: &mut Vec<LlmDelta>, value: &Value) {
    let Some(delta) = value.get("delta") else {
        return;
    };
    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => push_text(out, delta.get("text")),
        Some("input_json_delta") => out.push(LlmDelta::ToolCall {
            index,
            id: None,
            name: None,
            arguments: delta
                .get("partial_json")
                .and_then(Value::as_str)
                .map(str::to_string),
            replace_arguments: false,
        }),
        _ => {}
    }
}

fn push_anthropic_usage(out: &mut Vec<LlmDelta>, value: &Value) {
    let usage = value.get("usage").or_else(|| {
        value
            .get("message")
            .and_then(|message| message.get("usage"))
    });
    let Some(usage) = usage else {
        return;
    };
    out.push(LlmDelta::Usage {
        input_tokens: u32_field(usage, "input_tokens"),
        output_tokens: u32_field(usage, "output_tokens"),
    });
}

fn push_text(out: &mut Vec<LlmDelta>, value: Option<&Value>) {
    let Some(text) = value.and_then(Value::as_str) else {
        return;
    };
    if !text.is_empty() {
        out.push(LlmDelta::Text(text.to_string()));
    }
}

fn tool_delta(
    index: usize,
    id: Option<String>,
    name: Option<&Value>,
    arguments: Option<&Value>,
    replace_arguments: bool,
) -> LlmDelta {
    let arguments = arguments.map(|value| match value.as_str() {
        Some(text) => text.to_string(),
        None => value.to_string(),
    });
    LlmDelta::ToolCall {
        index,
        id,
        name: name.and_then(Value::as_str).map(str::to_string),
        arguments,
        replace_arguments,
    }
}

fn u32_field(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_tool_call_and_done() {
        let line = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"get_kernel_info","arguments":{}}}]},"done":false}"#;
        let deltas = parse_ollama_line(line).unwrap();
        assert!(matches!(
            &deltas[0],
            LlmDelta::ToolCall { name: Some(name), replace_arguments: true, .. } if name == "get_kernel_info"
        ));
        let done = parse_ollama_line(
            r#"{"message":{"content":""},"done":true,"prompt_eval_count":10,"eval_count":4}"#,
        )
        .unwrap();
        assert!(done.iter().any(|d| matches!(d, LlmDelta::Finished)));
    }

    #[test]
    fn ollama_error_is_not_content() {
        let err = parse_ollama_line(r#"{"error":"model not found"}"#).unwrap_err();
        assert!(err.to_string().contains("model not found"));
    }

    #[test]
    fn openai_sse_text_and_done() {
        let deltas =
            parse_openai_line(r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#).unwrap();
        assert_eq!(deltas, vec![LlmDelta::Text("Hello".into())]);
        let done = parse_openai_line("data: [DONE]").unwrap();
        assert_eq!(done, vec![LlmDelta::Finished]);
    }

    #[test]
    fn openai_ignores_non_data_lines() {
        assert!(parse_openai_line(": keep-alive").unwrap().is_empty());
    }

    #[test]
    fn anthropic_text_and_tool_stream() {
        let text = parse_anthropic_line(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
        )
        .unwrap();
        assert_eq!(text, vec![LlmDelta::Text("Hi".into())]);
        let start = parse_anthropic_line(
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_kernel_info","input":{}}}"#,
        )
        .unwrap();
        assert!(matches!(
            &start[0],
            LlmDelta::ToolCall { name: Some(name), .. } if name == "get_kernel_info"
        ));
        let partial = parse_anthropic_line(
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"k\":1}"}}"#,
        )
        .unwrap();
        assert!(matches!(partial[0], LlmDelta::ToolCall { .. }));
        let done = parse_anthropic_line(r#"data: {"type":"message_stop"}"#).unwrap();
        assert_eq!(done, vec![LlmDelta::Finished]);
        assert!(parse_anthropic_line("event: content_block_delta")
            .unwrap()
            .is_empty());
    }
}
