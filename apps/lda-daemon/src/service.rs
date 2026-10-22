//! Daemon state. SQLite and the keyring are touched here; the model is not trusted
//! to decide either one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use async_trait::async_trait;
use lda_common::{
    config::valid_provider_id, endpoint_is_local, new_id, now_ms, private_dir,
    read_to_string_limited, tighten_file, title_from, ConversationSummary, Error, FileConfig,
    Message, ProviderConfig, ProviderKind, Result, Role, SendReply, SourceLink, StoredMessage,
    StreamEvent, ToolRunRecord, MAX_SECRET_CHARS, MAX_USER_CHARS,
};
use lda_core::{run_turn, AgentEvent, AgentLimits, ConfirmHub, TurnRequest};
use lda_ipc::{AgentBackend, SignalBus};
use lda_llm::build_provider;
use lda_memory::{history_for_model, MemoryGate, Store};
use lda_security::{preview, resolve_api_key, PermissionPolicy, SecretStore};
use lda_tools::{agent_registry, ToolRegistry};
use lda_web::WebClient;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const MAX_INFLIGHT: usize = 8;

pub struct Service {
    this: Weak<Service>,
    config_path: PathBuf,
    config: Mutex<FileConfig>,
    store: Arc<Mutex<Store>>,
    ephemeral: Mutex<Ephemeral>,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    http_local: reqwest::Client,
    registry: ToolRegistry,
    policy: PermissionPolicy,
    web: Arc<WebClient>,
    memory: Arc<MemoryGate>,
    confirm: Arc<ConfirmHub>,
    inflight: Mutex<HashMap<String, CancellationToken>>,
    root: CancellationToken,
    signals: SignalBus,
    stop: Notify,
    secret_warned: AtomicBool,
}

struct Ephemeral {
    messages: HashMap<String, Vec<StoredMessage>>,
    summaries: HashMap<String, ConversationSummary>,
}

struct RequestGuard {
    service: Arc<Service>,
    id: String,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if let Ok(mut inflight) = self.service.inflight.lock() {
            inflight.remove(&self.id);
        }
    }
}

impl Service {
    pub fn open(
        config_path: PathBuf,
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Arc<Self>> {
        let config = load_config(&config_path)?;
        for provider in &config.providers {
            warn_if_remote(provider);
        }
        let store = Arc::new(Mutex::new(Store::open(&Store::path_of(&data_dir))?));
        {
            let guard = lock(&store)?;
            apply_retention(&guard, &config.agent)?;
        }
        let memory = Arc::new(MemoryGate::new(
            Arc::clone(&store),
            config.agent.persistent_memory,
        ));
        let web = Arc::new(WebClient::new(config.web.clone())?);
        let http = build_http(false)?;
        let http_local = build_http(true)?;
        let service = Arc::new_cyclic(|this| Self {
            this: this.clone(),
            config_path,
            config: Mutex::new(config),
            store,
            ephemeral: Mutex::new(Ephemeral {
                messages: HashMap::new(),
                summaries: HashMap::new(),
            }),
            secrets,
            http,
            http_local,
            registry: agent_registry(Arc::clone(&web), Arc::clone(&memory)),
            policy: PermissionPolicy::daemon(),
            web,
            memory,
            confirm: Arc::new(ConfirmHub::new()),
            inflight: Mutex::new(HashMap::new()),
            root: CancellationToken::new(),
            signals: SignalBus::default(),
            stop: Notify::new(),
            secret_warned: AtomicBool::new(false),
        });
        Ok(service)
    }

    pub fn signals(&self) -> &SignalBus {
        &self.signals
    }

    pub fn request_stop(&self) {
        self.root.cancel();
        self.stop.notify_one();
    }

    pub async fn stopped(&self) {
        self.stop.notified().await;
    }

    fn me(&self) -> Result<Arc<Self>> {
        self.this
            .upgrade()
            .ok_or_else(|| Error::Other("daemon is shutting down".into()))
    }

    fn persist(&self) -> Result<bool> {
        Ok(lock(&self.config)?.agent.persist_conversations)
    }

    fn provider_config(&self, id: &str) -> Result<ProviderConfig> {
        lock(&self.config)?
            .provider(id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("provider {id}")))
    }

    fn client_for(&self, provider: &ProviderConfig) -> reqwest::Client {
        if endpoint_is_local(&provider.endpoint) {
            return self.http_local.clone();
        }
        self.http.clone()
    }

    fn api_key(&self, provider: &ProviderConfig) -> Result<Option<String>> {
        let secrets = Arc::clone(&self.secrets);
        let id = provider.id.clone();
        let auth_env = provider.auth_env.clone();
        let local_without_env = provider.kind == ProviderKind::Ollama && auth_env.is_none();
        let resolved =
            blocking_secret(move || resolve_api_key(secrets.as_ref(), &id, auth_env.as_deref()));
        match resolved {
            Ok(value) => Ok(value),
            Err(err) if local_without_env => {
                self.warn_secret_once(&err);
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    fn warn_secret_once(&self, err: &Error) {
        if self.secret_warned.swap(true, Ordering::Relaxed) {
            return;
        }
        warn!(%err, "secret service is unavailable; local Ollama continues without a key");
    }

    fn ensure_conversation(
        &self,
        id: &str,
        provider: &ProviderConfig,
        text: &str,
    ) -> Result<String> {
        if id.is_empty() {
            let created = new_id();
            self.insert_conversation(&created, provider, text)?;
            return Ok(created);
        }
        if !valid_conversation_id(id) {
            return Err(Error::InvalidInput("conversation id is not valid".into()));
        }
        if self.conversation_exists(id)? {
            return Ok(id.to_string());
        }
        Err(Error::NotFound(id.to_string()))
    }

    fn conversation_exists(&self, id: &str) -> Result<bool> {
        if self.persist()? {
            return lock(&self.store)?.conversation_exists(id);
        }
        Ok(lock(&self.ephemeral)?.summaries.contains_key(id))
    }

    fn insert_conversation(&self, id: &str, provider: &ProviderConfig, text: &str) -> Result<()> {
        let now = now_ms();
        let title = title_from(text);
        if !self.persist()? {
            let summary = ConversationSummary {
                id: id.to_string(),
                title,
                provider_id: provider.id.clone(),
                model: provider.model.clone(),
                created_at: now,
                updated_at: now,
            };
            let mut memory = lock(&self.ephemeral)?;
            memory.summaries.insert(id.to_string(), summary);
            memory.messages.insert(id.to_string(), Vec::new());
            return Ok(());
        }
        lock(&self.store)?.create_conversation(id, &title, &provider.id, &provider.model)?;
        Ok(())
    }

    fn append_message(&self, conversation_id: &str, message: &Message) -> Result<()> {
        if message.role == Role::System {
            return Ok(());
        }
        let stored = StoredMessage {
            id: new_id(),
            conversation_id: conversation_id.to_string(),
            role: message.role,
            content: message.content.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
            tool_name: message.tool_name.clone(),
            created_at: now_ms(),
        };
        if !self.persist()? {
            return remember(&self.ephemeral, stored);
        }
        lock(&self.store)?.append_message(&stored)
    }

    fn history(&self, conversation_id: &str, limit: usize) -> Result<Vec<Message>> {
        if self.persist()? {
            let stored = lock(&self.store)?.recent_messages(conversation_id, limit)?;
            return Ok(history_for_model(&stored));
        }
        let memory = lock(&self.ephemeral)?;
        let messages = memory
            .messages
            .get(conversation_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        Ok(tail_messages(messages, limit))
    }

    fn record(&self, record: &ToolRunRecord) {
        if record.ok {
            info!(tool = %record.tool_name, "tool finished");
        } else {
            let err = record.error.as_deref().unwrap_or("");
            warn!(tool = %record.tool_name, error = %preview(err, 200), "tool failed");
        }
        if !self.persist().unwrap_or(false) {
            return;
        }
        if let Err(err) = lock(&self.store).and_then(|store| store.record_tool_run(record)) {
            warn!(%err, "could not store the tool audit record");
        }
    }

    fn spawn_turn(
        &self,
        conversation_id: String,
        provider: ProviderConfig,
        request_id: String,
    ) -> Result<()> {
        let cancel = self.root.child_token();
        {
            let mut inflight = lock(&self.inflight)?;
            if inflight.len() >= MAX_INFLIGHT {
                return Err(Error::InvalidInput(
                    "too many requests are already running".into(),
                ));
            }
            inflight.insert(request_id.clone(), cancel.clone());
        }
        let service = self.me()?;
        tokio::spawn(async move {
            let _guard = RequestGuard {
                service: Arc::clone(&service),
                id: request_id.clone(),
            };
            let result = service
                .run_request(conversation_id, provider, request_id.clone(), cancel)
                .await;
            if let Err(err) = result {
                report_failure(&service, &request_id, err).await;
            }
        });
        Ok(())
    }

    async fn run_request(
        self: &Arc<Self>,
        conversation_id: String,
        provider_cfg: ProviderConfig,
        request_id: String,
        cancel: CancellationToken,
    ) -> Result<()> {
        info!(conversation = %conversation_id, provider = %provider_cfg.id, "request started");
        let limits = self.limits()?;
        let history = self.history(&conversation_id, provider_cfg.context_messages)?;
        let key = self.api_key(&provider_cfg)?;
        let provider = build_provider(&provider_cfg, self.client_for(&provider_cfg), key);
        let memories = self.memory.notes_for_prompt();
        let mut rx = run_turn(TurnRequest {
            provider,
            provider_cfg,
            registry: self.registry.clone(),
            policy: self.policy.clone(),
            history,
            limits,
            cancel,
            conversation_id: conversation_id.clone(),
            request_id: request_id.clone(),
            confirm: Arc::clone(&self.confirm),
            memories,
            confirm_wait: Duration::from_secs(120),
        })
        .await;
        let mut accumulated = String::new();
        let mut saved_assistant = false;
        while let Some((event, record)) = rx.recv().await {
            let finished = self
                .on_event(
                    &conversation_id,
                    &request_id,
                    event,
                    record,
                    &mut accumulated,
                    &mut saved_assistant,
                )
                .await?;
            if finished {
                info!(request = %request_id, "request finished");
                return Ok(());
            }
        }
        self.signals
            .done(&request_id, "error", "response ended before completion")
            .await;
        Ok(())
    }

    async fn on_event(
        &self,
        conversation_id: &str,
        request_id: &str,
        event: AgentEvent,
        record: Option<ToolRunRecord>,
        accumulated: &mut String,
        saved_assistant: &mut bool,
    ) -> Result<bool> {
        match event {
            AgentEvent::Text(chunk) => publish_text(self, request_id, &chunk, accumulated).await,
            AgentEvent::ReplaceText(text) => {
                publish_replace(self, request_id, text, accumulated).await
            }
            AgentEvent::Tool(event) => publish_tool(self, request_id, event, record).await,
            AgentEvent::Persist(message) => {
                self.persist_event(conversation_id, message, saved_assistant)
            }
            AgentEvent::Done { text } => self.finish_ok(conversation_id, request_id, &text).await,
            AgentEvent::Failed { message } => {
                self.finish_failed(
                    conversation_id,
                    request_id,
                    &message,
                    accumulated,
                    *saved_assistant,
                )
                .await
            }
        }
    }

    fn persist_event(
        &self,
        conversation_id: &str,
        message: Message,
        saved_assistant: &mut bool,
    ) -> Result<bool> {
        if message.role == Role::Assistant {
            *saved_assistant = true;
        }
        self.append_message(conversation_id, &message)?;
        Ok(false)
    }

    async fn finish_ok(&self, conversation_id: &str, request_id: &str, text: &str) -> Result<bool> {
        if !text.is_empty() {
            self.append_message(conversation_id, &Message::text(Role::Assistant, text))?;
        }
        self.signals.done(request_id, "ok", "").await;
        Ok(true)
    }

    async fn finish_failed(
        &self,
        conversation_id: &str,
        request_id: &str,
        message: &str,
        accumulated: &str,
        saved_assistant: bool,
    ) -> Result<bool> {
        if message == "cancelled" && !saved_assistant && !accumulated.is_empty() {
            self.append_message(
                conversation_id,
                &Message::text(Role::Assistant, accumulated),
            )?;
        }
        let status = if message == "cancelled" {
            "cancelled"
        } else {
            "error"
        };
        self.signals
            .done(request_id, status, &preview(message, 500))
            .await;
        Ok(true)
    }

    fn limits(&self) -> Result<AgentLimits> {
        let agent = lock(&self.config)?.agent.clone();
        Ok(AgentLimits {
            max_iterations: agent.max_tool_iterations.clamp(1, 16),
            tool_timeout: Duration::from_secs(agent.tool_timeout_secs.max(1)),
        })
    }

    fn messages_json(&self, conversation_id: &str) -> Result<String> {
        if !valid_conversation_id(conversation_id) {
            return Err(Error::InvalidInput("conversation id is not valid".into()));
        }
        if !self.conversation_exists(conversation_id)? {
            return Err(Error::NotFound(conversation_id.to_string()));
        }
        if self.persist()? {
            let messages = lock(&self.store)?.recent_messages(conversation_id, 10_000)?;
            return to_json(&messages);
        }
        let memory = lock(&self.ephemeral)?;
        let messages = memory
            .messages
            .get(conversation_id)
            .cloned()
            .unwrap_or_default();
        to_json(&messages)
    }

    fn list_json(&self, query: &str) -> Result<String> {
        let query = bounded_text(query, 200, "search query")?;
        if self.persist()? {
            let rows = lock(&self.store)?.list_conversations(query)?;
            return to_json(&rows);
        }
        let memory = lock(&self.ephemeral)?;
        to_json(&ephemeral_rows(&memory, query))
    }

    fn export_json(&self) -> Result<String> {
        if self.persist()? {
            let value = lock(&self.store)?.export_json()?;
            return to_json(&value);
        }
        let memory = lock(&self.ephemeral)?;
        to_json(&serde_json::json!({ "conversations": ephemeral_export(&memory) }))
    }
}

#[async_trait]
impl AgentBackend for Service {
    fn signals(&self) -> &SignalBus {
        &self.signals
    }

    async fn ping(&self) -> String {
        "ok".to_string()
    }

    async fn send_message(
        &self,
        conversation_id: &str,
        provider_id: &str,
        text: &str,
    ) -> Result<String> {
        validate_user_text(text)?;
        let provider = self.provider_config(provider_id)?;
        let conversation_id = self.ensure_conversation(conversation_id, &provider, text)?;
        self.append_message(&conversation_id, &Message::text(Role::User, text))?;
        let request_id = new_id();
        let reply = SendReply {
            request_id: request_id.clone(),
            conversation_id: conversation_id.clone(),
        };
        self.spawn_turn(conversation_id, provider, request_id)?;
        to_json(&reply)
    }

    async fn cancel(&self, request_id: &str) -> Result<()> {
        let token = lock(&self.inflight)?.get(request_id).cloned();
        let Some(token) = token else {
            return Err(Error::NotFound(format!("request {request_id}")));
        };
        token.cancel();
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        self.request_stop();
        Ok(())
    }

    async fn list_conversations(&self, query: &str) -> Result<String> {
        self.list_json(query)
    }

    async fn get_messages(&self, conversation_id: &str) -> Result<String> {
        self.messages_json(conversation_id)
    }

    async fn delete_conversation(&self, conversation_id: &str) -> Result<()> {
        if !valid_conversation_id(conversation_id) {
            return Err(Error::InvalidInput("conversation id is not valid".into()));
        }
        if !self.persist()? {
            let mut memory = lock(&self.ephemeral)?;
            if memory.summaries.remove(conversation_id).is_none() {
                return Err(Error::NotFound(conversation_id.to_string()));
            }
            memory.messages.remove(conversation_id);
            return Ok(());
        }
        lock(&self.store)?.delete_conversation(conversation_id)
    }

    async fn delete_all_conversations(&self) -> Result<()> {
        if !self.persist()? {
            let mut memory = lock(&self.ephemeral)?;
            memory.messages.clear();
            memory.summaries.clear();
            return Ok(());
        }
        lock(&self.store)?.delete_all_conversations()
    }

    async fn list_providers(&self) -> Result<String> {
        let providers = lock(&self.config)?.providers.clone();
        to_json(&providers)
    }

    async fn upsert_provider(&self, provider_json: &str) -> Result<()> {
        let provider = parse_provider(provider_json)?;
        warn_if_remote(&provider);
        let mut config = lock(&self.config)?;
        config.upsert(provider)?;
        save_config(&self.config_path, &config)
    }

    async fn set_secret(&self, provider_id: &str, secret: &str) -> Result<()> {
        check_provider_id(provider_id)?;
        if secret.chars().count() > MAX_SECRET_CHARS
            || secret.trim().is_empty()
            || secret.contains('\0')
        {
            return Err(Error::InvalidInput("secret is empty or too long".into()));
        }
        let secrets = Arc::clone(&self.secrets);
        let id = provider_id.to_string();
        let secret = secret.to_string();
        blocking_secret(move || secrets.set(&id, &secret))
    }

    async fn clear_secret(&self, provider_id: &str) -> Result<()> {
        check_provider_id(provider_id)?;
        let secrets = Arc::clone(&self.secrets);
        let id = provider_id.to_string();
        blocking_secret(move || secrets.delete(&id))
    }

    async fn secret_status(&self, provider_id: &str) -> Result<String> {
        check_provider_id(provider_id)?;
        let secrets = Arc::clone(&self.secrets);
        let id = provider_id.to_string();
        let stored = blocking_secret(move || secrets.get(&id))?;
        if stored.is_some_and(|value| !value.is_empty()) {
            return Ok("store".to_string());
        }
        let env_name = lock(&self.config)?
            .provider(provider_id)
            .and_then(|provider| provider.auth_env.clone());
        if let Some(name) = env_name {
            if std::env::var(name)
                .ok()
                .is_some_and(|value| !value.is_empty())
            {
                return Ok("env".to_string());
            }
        }
        Ok("absent".to_string())
    }

    async fn list_models(&self, provider_id: &str) -> Result<String> {
        let provider = self.provider_config(provider_id)?;
        let key = self.api_key(&provider)?;
        let built = build_provider(&provider, self.client_for(&provider), key);
        let models = built.list_models().await?;
        to_json(&models)
    }

    async fn system_snapshot(&self) -> Result<String> {
        let value = tokio::task::spawn_blocking(lda_system::system_info)
            .await
            .map_err(|err| Error::Other(err.to_string()))?;
        Ok(value.to_string())
    }

    async fn reload_config(&self) -> Result<()> {
        let text = read_to_string_limited(&self.config_path, 1_000_000)?;
        let config = FileConfig::parse(&text)?;
        for provider in &config.providers {
            warn_if_remote(provider);
        }
        self.web.set_settings(config.web.clone()).await;
        self.memory.set_enabled(config.agent.persistent_memory);
        *lock(&self.config)? = config;
        Ok(())
    }

    async fn export_data(&self) -> Result<String> {
        self.export_json()
    }

    async fn confirm_action(&self, request_id: &str, call_id: &str, allow: bool) -> Result<()> {
        let key = confirm_key(request_id, call_id)?;
        if self.confirm.decide(&key, allow) {
            return Ok(());
        }
        Err(Error::NotFound("no confirmation is waiting".into()))
    }

    async fn list_memories(&self) -> Result<String> {
        to_json(&self.memory.list()?)
    }

    async fn remember(&self, text: &str) -> Result<String> {
        to_json(&self.memory.remember(text)?)
    }

    async fn forget_memory(&self, id: &str) -> Result<()> {
        self.memory.forget(id)
    }
}

async fn report_failure(service: &Service, request_id: &str, err: Error) {
    let status = if err.is_cancelled() {
        "cancelled"
    } else {
        "error"
    };
    let message = preview(&err.to_string(), 500);
    warn!(%status, error = %message, "request failed");
    service.signals.done(request_id, status, &message).await;
}

async fn publish_text(
    service: &Service,
    request_id: &str,
    chunk: &str,
    accumulated: &mut String,
) -> Result<bool> {
    accumulated.push_str(chunk);
    service.signals.delta(request_id, chunk).await;
    Ok(false)
}

async fn publish_replace(
    service: &Service,
    request_id: &str,
    text: String,
    accumulated: &mut String,
) -> Result<bool> {
    *accumulated = text.clone();
    service
        .signals
        .event(request_id, &StreamEvent::ReplaceText { text })
        .await;
    Ok(false)
}

async fn publish_tool(
    service: &Service,
    request_id: &str,
    event: StreamEvent,
    record: Option<ToolRunRecord>,
) -> Result<bool> {
    if let Some(record) = &record {
        service.record(record);
        let links = source_links(record.result_json.as_deref());
        if !links.is_empty() {
            service
                .signals
                .event(request_id, &StreamEvent::Sources { items: links })
                .await;
        }
    }
    notify_confirmation(&event).await;
    service.signals.event(request_id, &event).await;
    Ok(false)
}

async fn notify_confirmation(event: &StreamEvent) {
    let StreamEvent::Confirmation { name, summary, .. } = event else {
        return;
    };
    let body = format!("{name}: {summary}");
    if let Err(err) = lda_desktop::notify("Confirm an action", &body).await {
        warn!(%err, "confirmation notification was not shown");
    }
}

fn source_links(result_json: Option<&str>) -> Vec<SourceLink> {
    let Some(text) = result_json else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.get("sources").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for item in items {
        let Some(link) = one_source(item) else {
            continue;
        };
        links.push(link);
        if links.len() == 8 {
            break;
        }
    }
    links
}

fn one_source(item: &Value) -> Option<SourceLink> {
    let url = item.get("url")?.as_str()?;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    if url.chars().count() > 2_000 {
        return None;
    }
    let title = item.get("title").and_then(Value::as_str).unwrap_or(url);
    let kind = item
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("snippet");
    let kind = if kind == "page" { "page" } else { "snippet" };
    Some(SourceLink {
        title: preview(title, 180),
        url: url.to_string(),
        kind: kind.to_string(),
    })
}

fn confirm_key(request_id: &str, call_id: &str) -> Result<String> {
    if !plain_id(request_id) || !plain_id(call_id) {
        return Err(Error::InvalidInput("confirmation id is not valid".into()));
    }
    Ok(format!("{request_id}:{call_id}"))
}

fn plain_id(id: &str) -> bool {
    (1..=160).contains(&id.len()) && id.chars().all(|ch| ch.is_ascii() && !ch.is_ascii_control())
}

fn remember(ephemeral: &Mutex<Ephemeral>, message: StoredMessage) -> Result<()> {
    let mut memory = lock(ephemeral)?;
    if let Some(summary) = memory.summaries.get_mut(&message.conversation_id) {
        summary.updated_at = message.created_at;
    }
    memory
        .messages
        .entry(message.conversation_id.clone())
        .or_default()
        .push(message);
    Ok(())
}

fn tail_messages(messages: &[StoredMessage], limit: usize) -> Vec<Message> {
    let start = messages.len().saturating_sub(limit);
    let mut window = messages[start..].to_vec();
    while window
        .first()
        .is_some_and(|message| message.role == Role::Tool)
    {
        window.remove(0);
    }
    history_for_model(&window)
}

fn ephemeral_rows(memory: &Ephemeral, query: &str) -> Vec<ConversationSummary> {
    let mut rows = Vec::new();
    for summary in memory.summaries.values() {
        let messages = memory.messages.get(&summary.id);
        if ephemeral_match(summary, messages, query) {
            rows.push(summary.clone());
        }
    }
    rows.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
    rows.truncate(200);
    rows
}

fn ephemeral_export(memory: &Ephemeral) -> Vec<Value> {
    let mut exported = Vec::new();
    for summary in memory.summaries.values() {
        let messages = memory
            .messages
            .get(&summary.id)
            .cloned()
            .unwrap_or_default();
        exported.push(serde_json::json!({ "conversation": summary, "messages": messages }));
    }
    exported
}

fn ephemeral_match(
    summary: &ConversationSummary,
    messages: Option<&Vec<StoredMessage>>,
    query: &str,
) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let needle = query.to_lowercase();
    if summary.title.to_lowercase().contains(&needle) {
        return true;
    }
    messages
        .into_iter()
        .flatten()
        .any(|message| message.content.to_lowercase().contains(&needle))
}

fn validate_user_text(text: &str) -> Result<()> {
    if text.contains('\0') {
        return Err(Error::InvalidInput("message contains a NUL byte".into()));
    }
    if text.trim().is_empty() {
        return Err(Error::InvalidInput("message is empty".into()));
    }
    if text.chars().count() > MAX_USER_CHARS {
        return Err(Error::InvalidInput("message is too long".into()));
    }
    Ok(())
}

fn valid_conversation_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn check_provider_id(id: &str) -> Result<()> {
    if valid_provider_id(id) {
        return Ok(());
    }
    Err(Error::InvalidInput("provider id is not valid".into()))
}

fn bounded_text<'a>(text: &'a str, max: usize, label: &str) -> Result<&'a str> {
    if text.contains('\0') || text.chars().count() > max {
        return Err(Error::InvalidInput(format!(
            "{label} is too long or contains NUL"
        )));
    }
    Ok(text)
}

fn parse_provider(text: &str) -> Result<ProviderConfig> {
    if text.len() > 65_536 {
        return Err(Error::InvalidInput(
            "provider configuration is too large".into(),
        ));
    }
    serde_json::from_str(text).map_err(|err| Error::InvalidInput(err.to_string()))
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|err| Error::Other(err.to_string()))
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| Error::Other("lock poisoned".into()))
}

/// The keyring crate opens a blocking zbus connection. That calls
/// `Runtime::block_on`, which panics on a Tokio worker. A plain thread has
/// no runtime entered, so the lookup stays off the daemon's pool.
fn blocking_secret<T, F>(op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("lda-secret".into())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(op));
            let _ = tx.send(outcome);
        })
        .map_err(Error::from)?;
    match rx.recv() {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(Error::Other("secret service panicked".into())),
        Err(_) => Err(Error::Other("secret service thread stopped".into())),
    }
}

fn load_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        let config = FileConfig::default();
        save_config(path, &config)?;
        return Ok(config);
    }
    let text = read_to_string_limited(path, 1_000_000)?;
    FileConfig::parse(&text)
}

fn save_config(path: &Path, config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        private_dir(parent)?;
    }
    let text = config.to_toml()?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    tighten_file(&tmp)?;
    std::fs::rename(&tmp, path)?;
    tighten_file(path)?;
    Ok(())
}

fn apply_retention(store: &Store, settings: &lda_common::AgentSettings) -> Result<()> {
    let Some(days) = settings.retention_days else {
        return Ok(());
    };
    let millis = i64::from(days).saturating_mul(86_400_000);
    let cutoff = now_ms().saturating_sub(millis);
    let removed = store.purge_older_than(cutoff)?;
    if removed > 0 {
        info!(removed, "applied conversation retention");
    }
    Ok(())
}

fn warn_if_remote(provider: &ProviderConfig) {
    if provider.share_system_context && !endpoint_is_local(&provider.endpoint) {
        warn!(
            provider = %provider.id,
            "share_system_context is on for a remote endpoint; system tool results can leave this computer"
        );
    }
}

fn build_http(local_only: bool) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("linux-desktop-agent/0.2");
    if local_only {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|err| Error::Other(format!("http client: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lda_security::MemorySecretStore;

    #[tokio::test]
    async fn config_rejects_plaintext_secrets_and_stores_them_aside() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::open(
            dir.path().join("config.toml"),
            dir.path().join("data"),
            Arc::new(MemorySecretStore::default()),
        )
        .unwrap();
        let rejected = r#"{"id":"ollama","kind":"ollama","endpoint":"http://127.0.0.1:11434","model":"m","api_key":"secretsecret"}"#;
        assert!(service.upsert_provider(rejected).await.is_err());
        let accepted = r#"{"id":"ollama","kind":"ollama","endpoint":"http://127.0.0.1:11434","model":"mistral:latest","share_system_context":true}"#;
        service.upsert_provider(accepted).await.unwrap();
        let listed = service.list_providers().await.unwrap();
        assert!(listed.contains("mistral:latest"));
        assert!(!listed.contains("api_key"));
        service
            .set_secret("ollama", "supersecretvalue")
            .await
            .unwrap();
        let status = service.secret_status("ollama").await.unwrap();
        assert_eq!(status, "store");
        assert!(!status.contains("supersecret"));
        let snapshot = service.system_snapshot().await.unwrap();
        assert!(snapshot.contains("kernel_release"));
        assert!(service
            .delete_conversation("abcdef0123456789")
            .await
            .is_err());
        assert!(service.send_message("", "ollama", "   ").await.is_err());
        assert!(service
            .remember("The printer is on the desk.")
            .await
            .is_err());
        assert!(service
            .confirm_action("request", "call", true)
            .await
            .is_err());
        let links = source_links(Some(
            r#"{"sources":[{"title":"Docs","url":"https://example.com/a","kind":"snippet"},{"title":"x","url":"file:///etc/passwd","kind":"page"}]}"#,
        ));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/a");
    }
}
