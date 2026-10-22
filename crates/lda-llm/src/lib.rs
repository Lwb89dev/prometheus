//! Provider adapters. The agent loop depends on [`LlmProvider`], not on a vendor.
//!
//! Ollama and OpenAI-compatible HTTP are implemented. Adding a provider means
//! a new type that implements the trait and a match arm in [`build_provider`].

#![forbid(unsafe_code)]

mod delta;
mod http;
mod ollama;
mod openai;
mod parse;
mod provider;

#[cfg(any(test, feature = "test-util"))]
pub mod scripted;

pub use delta::{push_lines, LlmDelta, ToolCallBuffer};
pub use provider::{
    build_provider, ChatRequest, LlmProvider, ProviderCapabilities, ToolDefinition,
};
