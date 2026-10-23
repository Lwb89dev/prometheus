//! Types shared by every Linux Desktop Agent crate.
//!
//! This crate has no I/O beyond path helpers and does not talk to a model,
//! a bus, or the desktop.

#![forbid(unsafe_code)]

pub mod config;
pub mod endpoint;
pub mod error;
pub mod ids;
pub mod message;
pub mod models;
pub mod paths;
pub mod permission;

pub use config::{
    AgentSettings, FileConfig, ProviderConfig, ProviderKind, ToolCallingMode, WebProviderKind,
    WebSettings,
};
pub use endpoint::{endpoint_is_local, join_endpoint, parse_endpoint};
pub use error::{Error, Result};
pub use ids::{new_id, now_ms, title_from};
pub use message::{
    ConversationSummary, MemoryNote, Message, Role, SendReply, SourceLink, StoredMessage,
    StreamEvent, ToolCall, ToolRunRecord,
};
pub use models::{
    is_catalog_model, model_is_unset, rank_installed, suggest_for_ram, DiscoveryReport,
    InstalledModel, ModelSuggestion,
};
pub use paths::{private_dir, read_to_string_limited, tighten_file, xdg_config_dir, xdg_data_dir};
pub use permission::{ConfirmationPolicy, PermissionLevel, Risk};

pub const BUS_NAME: &str = "org.linuxdesktopagent.Daemon";
pub const BUS_PATH: &str = "/org/linuxdesktopagent/Agent";
pub const BUS_INTERFACE: &str = "org.linuxdesktopagent.Agent1";
pub const GTK_APP_ID: &str = "org.linuxdesktopagent.Gtk";
pub const SECRET_SERVICE_NAME: &str = "linux-desktop-agent";

pub const MAX_USER_CHARS: usize = 32_000;
pub const MAX_TOOL_OUTPUT: usize = 16_384;
pub const MAX_STORED_MESSAGE: usize = 256_000;
pub const MAX_SECRET_CHARS: usize = 4_096;
