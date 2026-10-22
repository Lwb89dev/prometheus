use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lda_common::{ConfirmationPolicy, PermissionLevel, Result, Risk};
use lda_llm::ToolDefinition;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::schema::validate_arguments;

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub permission: PermissionLevel,
    pub risk: Risk,
    pub timeout: Duration,
    pub confirmation: ConfirmationPolicy,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> &ToolSpec;
    async fn execute(&self, arguments: &Value, cancel: &CancellationToken) -> Result<Value>;
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|tool| tool.spec().name == name)
            .cloned()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|tool| {
                let spec = tool.spec();
                ToolDefinition {
                    name: spec.name.to_string(),
                    description: spec.description.to_string(),
                    parameters: spec.parameters.clone(),
                }
            })
            .collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|tool| tool.spec().name).collect()
    }
}

pub fn prepare_call(tool: &dyn Tool, arguments: &Value) -> Result<()> {
    let name = tool.spec().name;
    if !valid_tool_name(name) {
        return Err(lda_common::Error::InvalidInput(format!(
            "tool name {name} is not valid"
        )));
    }
    validate_arguments(&tool.spec().parameters, arguments)
}

pub fn valid_tool_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    first.is_ascii_lowercase() && rest.len() <= 63 && rest.iter().copied().all(tool_name_byte)
}

fn tool_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
}
