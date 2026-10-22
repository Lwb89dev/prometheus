use lda_common::{new_id, ToolCall};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum LlmDelta {
    Text(String),
    ToolCall {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
        replace_arguments: bool,
    },
    Usage {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
    Finished,
}

#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
pub struct ToolCallBuffer {
    calls: Vec<PartialCall>,
}

impl ToolCallBuffer {
    pub fn push(&mut self, delta: &LlmDelta) {
        let LlmDelta::ToolCall {
            index,
            id,
            name,
            arguments,
            replace_arguments,
        } = delta
        else {
            return;
        };
        while self.calls.len() <= *index {
            self.calls.push(PartialCall::default());
        }
        let slot = &mut self.calls[*index];
        if let Some(id) = id {
            if !id.is_empty() {
                slot.id.clone_from(id);
            }
        }
        if let Some(name) = name {
            if !name.is_empty() {
                slot.name.clone_from(name);
            }
        }
        if let Some(arguments) = arguments {
            if *replace_arguments {
                slot.arguments.clone_from(arguments);
            } else {
                slot.arguments.push_str(arguments);
            }
        }
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .filter(|call| !call.name.is_empty())
            .map(|call| ToolCall {
                id: if call.id.is_empty() {
                    new_id()
                } else {
                    call.id
                },
                name: call.name,
                arguments: parse_arguments(&call.arguments),
            })
            .collect()
    }
}

fn parse_arguments(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| Value::String(trimmed.to_string()))
}

pub fn push_lines(buffer: &mut String, chunk: &str) -> Vec<String> {
    buffer.push_str(chunk);
    let mut lines = Vec::new();
    while let Some(index) = buffer.find('\n') {
        let mut line: String = buffer.drain(..=index).collect();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_fragments_become_one_call() {
        let mut buffer = ToolCallBuffer::default();
        buffer.push(&LlmDelta::ToolCall {
            index: 0,
            id: Some("call_1".into()),
            name: Some("get_memory_info".into()),
            arguments: Some("{\"uni".into()),
            replace_arguments: false,
        });
        buffer.push(&LlmDelta::ToolCall {
            index: 0,
            id: None,
            name: None,
            arguments: Some("t\":\"kb\"}".into()),
            replace_arguments: false,
        });
        let calls = buffer.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_memory_info");
        assert_eq!(calls[0].arguments["unit"], "kb");
    }

    #[test]
    fn lines_split_across_chunks() {
        let mut buf = String::new();
        assert!(push_lines(&mut buf, "{\"a\":1").is_empty());
        let lines = push_lines(&mut buf, "}\n{\"b\":2}\n");
        assert_eq!(lines.len(), 2);
    }
}
