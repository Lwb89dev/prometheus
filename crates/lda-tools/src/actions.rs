//! User actions that stay inside the home directory or open a confirmed URI.
//! Package install, service control, and polkit are not implemented.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lda_common::{ConfirmationPolicy, PermissionLevel, Result, Risk};
use lda_desktop::open_uri;
use lda_memory::MemoryGate;
use lda_security::resolve_within;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::registry::{Tool, ToolSpec};

pub fn action_tools(memory: Arc<MemoryGate>) -> Vec<Arc<dyn Tool>> {
    vec![
        tool(
            meta(
                "locate_file",
                "Find a file name under the home directory. Does not read file contents.",
                json!({
                    "type": "object",
                    "properties": {"name": {"type": "string", "maxLength": 180}},
                    "required": ["name"],
                    "additionalProperties": false
                }),
                PermissionLevel::ReadOnly,
                ConfirmationPolicy::Never,
                Risk::Low,
            ),
            Kind::Locate,
            None,
        ),
        tool(
            meta(
                "open_uri",
                "Open an http(s) URI in the default application after the user confirms.",
                json!({
                    "type": "object",
                    "properties": {"uri": {"type": "string", "maxLength": 2000}},
                    "required": ["uri"],
                    "additionalProperties": false
                }),
                PermissionLevel::UserAction,
                ConfirmationPolicy::Always,
                Risk::Medium,
            ),
            Kind::OpenUri,
            None,
        ),
        tool(
            meta(
                "open_directory",
                "Open a directory inside the home folder after the user confirms.",
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "maxLength": 500}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
                PermissionLevel::UserAction,
                ConfirmationPolicy::Always,
                Risk::Medium,
            ),
            Kind::OpenDir,
            None,
        ),
        tool(
            meta(
                "open_application",
                "Launch a desktop application id such as firefox after the user confirms.",
                json!({
                    "type": "object",
                    "properties": {"app_id": {"type": "string", "maxLength": 80}},
                    "required": ["app_id"],
                    "additionalProperties": false
                }),
                PermissionLevel::UserAction,
                ConfirmationPolicy::Always,
                Risk::Medium,
            ),
            Kind::OpenApp,
            None,
        ),
        tool(
            meta(
                "list_memories",
                "List notes the user explicitly saved. This does not infer new notes.",
                empty_object(),
                PermissionLevel::ReadOnly,
                ConfirmationPolicy::Never,
                Risk::Low,
            ),
            Kind::ListMemory,
            Some(Arc::clone(&memory)),
        ),
        tool(
            meta(
                "remember",
                "Store one note the user has confirmed. Do not store secrets.",
                json!({
                    "type": "object",
                    "properties": {"text": {"type": "string", "maxLength": 2000}},
                    "required": ["text"],
                    "additionalProperties": false
                }),
                PermissionLevel::UserAction,
                ConfirmationPolicy::Always,
                Risk::Medium,
            ),
            Kind::Remember,
            Some(Arc::clone(&memory)),
        ),
        tool(
            meta(
                "forget_memory",
                "Delete one saved note by id after the user confirms.",
                json!({
                    "type": "object",
                    "properties": {"id": {"type": "string", "maxLength": 32}},
                    "required": ["id"],
                    "additionalProperties": false
                }),
                PermissionLevel::UserAction,
                ConfirmationPolicy::Always,
                Risk::Medium,
            ),
            Kind::Forget,
            Some(memory),
        ),
    ]
}

struct Meta {
    name: &'static str,
    description: &'static str,
    parameters: Value,
    permission: PermissionLevel,
    confirmation: ConfirmationPolicy,
    risk: Risk,
}

fn meta(
    name: &'static str,
    description: &'static str,
    parameters: Value,
    permission: PermissionLevel,
    confirmation: ConfirmationPolicy,
    risk: Risk,
) -> Meta {
    Meta {
        name,
        description,
        parameters,
        permission,
        confirmation,
        risk,
    }
}

fn empty_object() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}

#[derive(Clone, Copy)]
enum Kind {
    Locate,
    OpenUri,
    OpenDir,
    OpenApp,
    ListMemory,
    Remember,
    Forget,
}

struct FuncTool {
    spec: ToolSpec,
    kind: Kind,
    memory: Option<Arc<MemoryGate>>,
}

fn tool(meta: Meta, kind: Kind, memory: Option<Arc<MemoryGate>>) -> Arc<dyn Tool> {
    Arc::new(FuncTool {
        spec: ToolSpec {
            name: meta.name,
            description: meta.description,
            parameters: meta.parameters,
            permission: meta.permission,
            risk: meta.risk,
            timeout: Duration::from_secs(20),
            confirmation: meta.confirmation,
        },
        kind,
        memory,
    })
}

#[async_trait]
impl Tool for FuncTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn execute(&self, arguments: &Value, _cancel: &CancellationToken) -> Result<Value> {
        let arguments = arguments.clone();
        let memory = self.memory.clone();
        match self.kind {
            Kind::Locate => blocking(move || locate(&arguments)).await,
            Kind::OpenUri => open_http(&arguments).await,
            Kind::OpenDir => open_dir(&arguments).await,
            Kind::OpenApp => blocking(move || open_app(&arguments)).await,
            Kind::ListMemory => list_memory(memory.as_ref()),
            Kind::Remember => remember(memory.as_ref(), &arguments),
            Kind::Forget => forget(memory.as_ref(), &arguments),
        }
    }
}

async fn blocking<T>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| lda_common::Error::Other(err.to_string()))?
}

fn locate(arguments: &Value) -> Result<Value> {
    let name = arguments.get("name").and_then(Value::as_str).unwrap_or("");
    if !valid_file_name(name) {
        return Err(lda_common::Error::InvalidInput(
            "file name must be a single path segment".into(),
        ));
    }
    let home = home_dir()?;
    let matches = walk_name(&home, name, 0, &mut 0);
    Ok(json!({"kind": "observation", "matches": matches}))
}

fn walk_name(dir: &Path, name: &str, depth: u8, found: &mut usize) -> Vec<String> {
    if depth >= 5 || *found >= 20 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if *found >= 20 {
            break;
        }
        let file_name = entry.file_name();
        let Some(text) = file_name.to_str() else {
            continue;
        };
        if text.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if text == name {
            out.push(path.display().to_string());
            *found += 1;
        }
        if kind.is_dir() {
            out.extend(walk_name(&path, name, depth + 1, found));
        }
    }
    out
}

async fn open_http(arguments: &Value) -> Result<Value> {
    let uri = arguments.get("uri").and_then(Value::as_str).unwrap_or("");
    if !(uri.starts_with("https://") || uri.starts_with("http://")) {
        return Err(lda_common::Error::InvalidInput(
            "open_uri accepts only http and https".into(),
        ));
    }
    open_uri(uri).await?;
    Ok(json!({"opened": uri}))
}

async fn open_dir(arguments: &Value) -> Result<Value> {
    let raw = arguments.get("path").and_then(Value::as_str).unwrap_or("");
    let home = home_dir()?;
    let path = resolve_within(&home, Path::new(raw))?;
    if !path.is_dir() {
        return Err(lda_common::Error::NotFound(
            "that directory is not inside the home folder".into(),
        ));
    }
    let url = url::Url::from_file_path(&path).map_err(|_| {
        lda_common::Error::InvalidInput("the directory cannot be expressed as a file URI".into())
    })?;
    open_uri(url.as_str()).await?;
    Ok(json!({"opened": path.display().to_string()}))
}

fn open_app(arguments: &Value) -> Result<Value> {
    let app_id = arguments
        .get("app_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !valid_app_id(app_id) {
        return Err(lda_common::Error::InvalidInput(
            "app_id must look like a desktop file name".into(),
        ));
    }
    let status = Command::new("gtk-launch")
        .arg(app_id)
        .status()
        .map_err(|err| lda_common::Error::Other(format!("gtk-launch: {err}")))?;
    if !status.success() {
        return Err(lda_common::Error::Other(format!(
            "gtk-launch exited with {status}"
        )));
    }
    Ok(json!({"launched": app_id}))
}

fn list_memory(memory: Option<&Arc<MemoryGate>>) -> Result<Value> {
    let memory = memory.ok_or_else(|| lda_common::Error::Other("memory is unavailable".into()))?;
    let notes = memory.list()?;
    Ok(json!({"notes": notes}))
}

fn remember(memory: Option<&Arc<MemoryGate>>, arguments: &Value) -> Result<Value> {
    let memory = memory.ok_or_else(|| lda_common::Error::Other("memory is unavailable".into()))?;
    let text = arguments.get("text").and_then(Value::as_str).unwrap_or("");
    let note = memory.remember(text)?;
    Ok(json!({"stored": note}))
}

fn forget(memory: Option<&Arc<MemoryGate>>, arguments: &Value) -> Result<Value> {
    let memory = memory.ok_or_else(|| lda_common::Error::Other("memory is unavailable".into()))?;
    let id = arguments.get("id").and_then(Value::as_str).unwrap_or("");
    memory.forget(id)?;
    Ok(json!({"deleted": id}))
}

fn home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        lda_common::Error::InvalidInput("HOME is not set, so file tools are unavailable".into())
    })?;
    Ok(PathBuf::from(home))
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && !name.contains('/')
        && !name.contains('\0')
        && name != "."
        && name != ".."
}

fn valid_app_id(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && rest.len() <= 80
        && rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_and_app_names_are_narrow() {
        assert!(valid_file_name("notes.txt"));
        assert!(!valid_file_name("../passwd"));
        assert!(!valid_file_name("a/b"));
        assert!(valid_app_id("firefox"));
        assert!(!valid_app_id("firefox;rm"));
    }
}
