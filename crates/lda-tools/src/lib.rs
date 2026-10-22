//! Allowlisted tools. The model can name a tool; it cannot invent a new one.
//!
//! Milestone 1 registers read-only system tools. The daemon registry also
//! registers web tools and confirmed user actions. The policy still decides
//! which of those may run.

#![forbid(unsafe_code)]

mod actions;
mod registry;
mod schema;
mod system;
mod web;

pub use actions::action_tools;
pub use registry::{prepare_call, valid_tool_name, Tool, ToolRegistry, ToolSpec};
pub use schema::validate_arguments;
pub use system::system_registry;
pub use web::web_tools;

use std::sync::Arc;

use lda_memory::MemoryGate;
use lda_web::WebClient;

/// System tools plus web and confirmed actions. The CLI keeps [`system_registry`].
pub fn agent_registry(web: Arc<WebClient>, memory: Arc<MemoryGate>) -> ToolRegistry {
    let mut registry = system_registry();
    registry.append(web_tools(web));
    registry.append(action_tools(memory));
    registry
}
