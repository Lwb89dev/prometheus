//! The agent loop. Permissions and tool allowlists are applied here, in Rust,
//! not by asking the model to behave.

#![forbid(unsafe_code)]

mod agent;
mod prompt;

pub use agent::{offered_tools, run_turn, AgentEvent, AgentLimits, TurnRequest};
pub use prompt::{extract_tool_fence, system_prompt};
