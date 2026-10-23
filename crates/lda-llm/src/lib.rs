//! Provider adapters. The agent loop depends on [`LlmProvider`], not on a vendor.
//!
//! Ollama, OpenAI-compatible HTTP, xAI (OpenAI-compatible), and Anthropic are
//! implemented. Adding a provider means a new type that implements the trait
//! and a match arm in [`build_provider`].

#![forbid(unsafe_code)]

mod anthropic;
mod audio;
mod delta;
mod http;
mod ollama;
mod openai;
mod parse;
mod provider;

#[cfg(any(test, feature = "test-util"))]
pub mod scripted;

pub use audio::{synthesize_speech, transcribe_audio};
pub use delta::{push_lines, LlmDelta, ToolCallBuffer};
pub use ollama::{apply_show, models_from_tags, pull_model, OllamaProvider};
pub use provider::{
    build_provider, ChatRequest, LlmProvider, ProviderCapabilities, ToolDefinition,
};
