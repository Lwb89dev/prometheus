use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Ollama,
    OpenaiCompatible,
    Anthropic,
    Xai,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Ollama => "ollama",
            ProviderKind::OpenaiCompatible => "openai_compatible",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Xai => "xai",
        }
    }
}

/// Where web search is allowed to send a query.
///
/// `searxng` is for an instance you run. A remote SearXNG host is rejected
/// unless `allow_remote_search` is set. `json` is for an API whose response
/// uses the same `results` array. There is no default public endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebProviderKind {
    #[default]
    None,
    Searxng,
    Json,
}

/// How tool calls are requested from the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallingMode {
    /// Native tool calls when the provider accepts them, plus the text fallback
    /// when the model emits a `tool` fence and no native call.
    #[default]
    Auto,
    Native,
    Text,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_context_messages")]
    pub context_messages: usize,
    #[serde(default)]
    pub context_window: Option<u32>,
    /// When false, system tools are not offered to a non-loopback provider.
    #[serde(default)]
    pub share_system_context: bool,
    #[serde(default)]
    pub tool_calling: ToolCallingMode,
    /// Name of an environment variable that holds the API key.
    /// The value itself is never written to this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
}

fn default_timeout() -> u64 {
    180
}

fn default_context_messages() -> usize {
    20
}

fn default_iterations() -> u32 {
    4
}

fn default_tool_timeout() -> u64 {
    20
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentSettings {
    #[serde(default = "default_iterations")]
    pub max_tool_iterations: u32,
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout_secs: u64,
    #[serde(default = "default_true")]
    pub persist_conversations: bool,
    /// Delete conversations older than this many days on startup. `None` keeps them.
    #[serde(default)]
    pub retention_days: Option<u32>,
    /// User-saved notes. The assistant does not invent these.
    #[serde(default)]
    pub persistent_memory: bool,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_iterations(),
            tool_timeout_secs: default_tool_timeout(),
            persist_conversations: true,
            retention_days: None,
            persistent_memory: false,
        }
    }
}

fn default_web_results() -> u8 {
    5
}

fn default_web_timeout() -> u64 {
    20
}

fn default_web_interval() -> u64 {
    2_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub kind: WebProviderKind,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
    #[serde(default = "default_web_results")]
    pub max_results: u8,
    #[serde(default = "default_web_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_web_interval")]
    pub min_interval_ms: u64,
    /// Lets SearXNG point at a non-loopback host. Off by default.
    #[serde(default)]
    pub allow_remote_search: bool,
    /// Lets page fetch contact private and loopback addresses. Off by default.
    #[serde(default)]
    pub allow_private_fetch: bool,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: WebProviderKind::None,
            endpoint: String::new(),
            auth_env: None,
            max_results: default_web_results(),
            timeout_secs: default_web_timeout(),
            min_interval_ms: default_web_interval(),
            allow_remote_search: false,
            allow_private_fetch: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub web: WebSettings,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            agent: AgentSettings::default(),
            web: WebSettings::default(),
            providers: vec![ProviderConfig {
                id: "ollama".to_string(),
                kind: ProviderKind::Ollama,
                endpoint: DEFAULT_OLLAMA_ENDPOINT.to_string(),
                model: "mistral:latest".to_string(),
                temperature: Some(0.2),
                timeout_secs: default_timeout(),
                context_messages: default_context_messages(),
                context_window: Some(8192),
                share_system_context: true,
                tool_calling: ToolCallingMode::Auto,
                auth_env: None,
            }],
        }
    }
}

impl FileConfig {
    pub fn parse(text: &str) -> Result<Self> {
        let config: FileConfig =
            toml::from_str(text).map_err(|e| Error::InvalidInput(e.to_string()))?;
        for provider in &config.providers {
            validate_provider(provider)?;
        }
        if config.agent.max_tool_iterations == 0 || config.agent.max_tool_iterations > 16 {
            return Err(Error::InvalidInput(
                "max_tool_iterations must be between 1 and 16".into(),
            ));
        }
        validate_web(&config.web)?;
        Ok(config)
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error::Other(e.to_string()))
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        if id.is_empty() {
            return self.providers.first();
        }
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn upsert(&mut self, provider: ProviderConfig) -> Result<()> {
        validate_provider(&provider)?;
        if let Some(existing) = self.providers.iter_mut().find(|p| p.id == provider.id) {
            *existing = provider;
        } else {
            self.providers.push(provider);
        }
        Ok(())
    }
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    if !valid_provider_id(&provider.id) {
        return Err(Error::InvalidInput(
            "provider id must match ^[a-z][a-z0-9_-]{0,63}$".into(),
        ));
    }
    crate::endpoint::parse_endpoint(&provider.endpoint)?;
    if provider.model.trim().is_empty() || provider.model.len() > 256 {
        return Err(Error::InvalidInput("model id is empty or too long".into()));
    }
    if provider.model.chars().any(|c| c.is_control()) {
        return Err(Error::InvalidInput(
            "model id contains control characters".into(),
        ));
    }
    if let Some(temp) = provider.temperature {
        if !(0.0..=2.0).contains(&temp) {
            return Err(Error::InvalidInput(
                "temperature must be between 0 and 2".into(),
            ));
        }
    }
    if provider.timeout_secs == 0 || provider.timeout_secs > 3600 {
        return Err(Error::InvalidInput(
            "timeout_secs must be between 1 and 3600".into(),
        ));
    }
    if provider.context_messages == 0 || provider.context_messages > 200 {
        return Err(Error::InvalidInput(
            "context_messages must be between 1 and 200".into(),
        ));
    }
    if let Some(name) = &provider.auth_env {
        if !valid_env_name(name) {
            return Err(Error::InvalidInput(
                "auth_env is not an environment variable name".into(),
            ));
        }
    }
    Ok(())
}

pub fn valid_provider_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    first.is_ascii_lowercase() && rest.len() <= 63 && rest.iter().copied().all(provider_id_byte)
}

fn provider_id_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
}

pub fn validate_web(web: &WebSettings) -> Result<()> {
    if web.max_results == 0 || web.max_results > 8 {
        return Err(Error::InvalidInput(
            "web.max_results must be between 1 and 8".into(),
        ));
    }
    if web.timeout_secs == 0 || web.timeout_secs > 60 {
        return Err(Error::InvalidInput(
            "web.timeout_secs must be between 1 and 60".into(),
        ));
    }
    if web.min_interval_ms > 60_000 {
        return Err(Error::InvalidInput(
            "web.min_interval_ms must be at most 60000".into(),
        ));
    }
    if let Some(name) = &web.auth_env {
        if !valid_env_name(name) {
            return Err(Error::InvalidInput(
                "web.auth_env is not an environment variable name".into(),
            ));
        }
    }
    if !web.enabled {
        return Ok(());
    }
    if matches!(web.kind, WebProviderKind::None) {
        return Err(Error::InvalidInput(
            "web.kind must be searxng or json when web search is enabled".into(),
        ));
    }
    let url = crate::endpoint::parse_endpoint(&web.endpoint)?;
    if matches!(web.kind, WebProviderKind::Searxng)
        && !web.allow_remote_search
        && !crate::endpoint::endpoint_is_local(&web.endpoint)
    {
        return Err(Error::InvalidInput(
            "SearXNG must be a loopback instance unless allow_remote_search is true".into(),
        ));
    }
    let _ = url;
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && name.len() <= 128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let config = FileConfig::default();
        let text = config.to_toml().unwrap();
        assert!(!text.contains("api_key"));
        let parsed = FileConfig::parse(&text).unwrap();
        assert_eq!(parsed.providers[0].id, "ollama");
        assert!(parsed.providers[0].share_system_context);
    }

    #[test]
    fn rejects_plaintext_api_key_field() {
        let text = r#"
            [[providers]]
            id = "openai"
            kind = "openai_compatible"
            endpoint = "https://api.openai.com/v1"
            model = "gpt-4.1-mini"
            api_key = "sk-secretsecretsecret"
        "#;
        let err = FileConfig::parse(text).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("api_key") || msg.contains("unknown"));
    }

    #[test]
    fn rejects_credentials_in_endpoint() {
        let text = r#"
            [[providers]]
            id = "bad"
            kind = "ollama"
            endpoint = "http://user:secret@127.0.0.1:11434"
            model = "mistral:latest"
        "#;
        assert!(FileConfig::parse(text).is_err());
    }

    #[test]
    fn searxng_must_be_local_unless_opted_in() {
        let remote = r#"
            [web]
            enabled = true
            kind = "searxng"
            endpoint = "https://searx.example/search"
        "#;
        assert!(FileConfig::parse(remote).is_err());
        let local = r#"
            [web]
            enabled = true
            kind = "searxng"
            endpoint = "http://127.0.0.1:8080"
        "#;
        assert!(FileConfig::parse(local).is_ok());
    }
}
