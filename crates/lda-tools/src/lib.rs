//! Allowlisted tools. The model can name a tool; it cannot invent a new one.
//!
//! Milestone 1 registers read-only system tools. Higher permission levels are
//! rejected by [`lda_security::PermissionPolicy`] even if a tool were registered.

#![forbid(unsafe_code)]

mod registry;
mod schema;
mod system;

pub use registry::{prepare_call, valid_tool_name, Tool, ToolRegistry, ToolSpec};
pub use schema::validate_arguments;
pub use system::system_registry;
