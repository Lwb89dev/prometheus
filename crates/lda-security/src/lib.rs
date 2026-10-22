//! Security checks that do not depend on a model.
//!
//! Permission decisions, path confinement, secret storage, and log redaction
//! live here so the agent loop can call them without trusting prompt text.

#![forbid(unsafe_code)]

mod pathutil;
mod policy;
mod redact;
mod secrets;

pub use pathutil::resolve_within;
pub use policy::PermissionPolicy;
pub use redact::{preview, redact_secrets};
pub use secrets::{resolve_api_key, KeyringSecretStore, MemorySecretStore, SecretStore};
