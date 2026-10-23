//! Owns the D-Bus connection on the Tokio runtime. The GTK thread only sees [`UiMsg`].

use std::path::PathBuf;
use std::process::{Command as Process, Stdio};
use std::time::Duration;

use futures_util::StreamExt;
use lda_common::{
    private_dir, xdg_data_dir, ConversationSummary, DiscoveryReport, MemoryNote, ProviderConfig,
    SendReply, StoredMessage, StreamEvent,
};
use lda_ipc::{session_proxy, AgentProxy, BusStreamEvent, StreamDelta, StreamDone};

#[derive(Clone)]
pub struct Launch {
    pub config: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

pub enum Command {
    Send {
        conversation_id: String,
        provider_id: String,
        text: String,
    },
    Cancel {
        request_id: String,
    },
    ListConversations {
        query: String,
    },
    GetMessages {
        conversation_id: String,
    },
    DeleteConversation {
        id: String,
    },
    DeleteAll,
    ListProviders,
    UpsertProvider {
        json: String,
    },
    SetSecret {
        id: String,
        secret: String,
    },
    ClearSecret {
        id: String,
    },
    SecretStatus {
        id: String,
    },
    ListModels {
        id: String,
    },
    Discover {
        id: String,
    },
    StartPull {
        id: String,
        name: String,
    },
    PullStatus,
    Shutdown,
    Confirm {
        request_id: String,
        call_id: String,
        allow: bool,
    },
    ListMemories,
    ForgetMemory {
        id: String,
    },
}

pub enum UiMsg {
    Connected,
    Status(String),
    RequestFailed(String),
    SendStarted {
        request_id: String,
        conversation_id: String,
    },
    Delta {
        request_id: String,
        text: String,
    },
    Event {
        request_id: String,
        event: StreamEvent,
    },
    Done {
        request_id: String,
        status: String,
        error: String,
    },
    Conversations(Vec<ConversationSummary>),
    Messages {
        conversation_id: String,
        messages: Vec<StoredMessage>,
    },
    Providers(Vec<ProviderConfig>),
    Models(Vec<String>),
    Discovery(DiscoveryReport),
    PullProgress {
        phase: String,
        name: String,
        detail: String,
    },
    SecretStatus(String),
    Deleted {
        id: String,
    },
    DeletedAll,
    ShutdownDone,
    Memories(Vec<MemoryNote>),
}

pub async fn run(
    cmd_rx: async_channel::Receiver<Command>,
    ui: async_channel::Sender<UiMsg>,
    launch: Launch,
) {
    let proxy = match connect(&launch).await {
        Ok(proxy) => proxy,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err)).await;
            return;
        }
    };
    let streams = match subscribe(&proxy).await {
        Ok(streams) => streams,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err)).await;
            return;
        }
    };
    let _ = ui.send(UiMsg::Connected).await;
    command_loop(cmd_rx, ui, proxy, streams).await;
}

struct Streams {
    deltas: lda_ipc::StreamDeltaStream,
    events: lda_ipc::StreamEventStream,
    dones: lda_ipc::StreamDoneStream,
}

async fn subscribe(proxy: &AgentProxy<'_>) -> Result<Streams, String> {
    let deltas = proxy
        .receive_stream_delta()
        .await
        .map_err(|err| err.to_string())?;
    let events = proxy
        .receive_stream_event()
        .await
        .map_err(|err| err.to_string())?;
    let dones = proxy
        .receive_stream_done()
        .await
        .map_err(|err| err.to_string())?;
    Ok(Streams {
        deltas,
        events,
        dones,
    })
}

async fn command_loop(
    cmd_rx: async_channel::Receiver<Command>,
    ui: async_channel::Sender<UiMsg>,
    proxy: AgentProxy<'static>,
    mut streams: Streams,
) {
    let mut streams_open = true;
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Ok(cmd) = cmd else { break };
                handle_command(&proxy, &ui, cmd).await;
            }
            item = streams.deltas.next(), if streams_open => {
                streams_open = forward_delta(item, &ui).await && streams_open;
            }
            item = streams.events.next(), if streams_open => {
                streams_open = forward_event(item, &ui).await && streams_open;
            }
            item = streams.dones.next(), if streams_open => {
                streams_open = forward_done(item, &ui).await && streams_open;
            }
        }
    }
}

async fn handle_command(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>, cmd: Command) {
    match cmd {
        Command::Send {
            conversation_id,
            provider_id,
            text,
        } => {
            send_message(proxy, ui, &conversation_id, &provider_id, &text).await;
        }
        Command::Cancel { request_id } => {
            send_unit(ui, proxy.cancel(&request_id).await, "Stop sent.").await;
        }
        Command::ListConversations { query } => list_conversations(proxy, ui, &query).await,
        Command::GetMessages { conversation_id } => get_messages(proxy, ui, &conversation_id).await,
        Command::DeleteConversation { id } => delete_conversation(proxy, ui, &id).await,
        Command::DeleteAll => delete_all(proxy, ui).await,
        Command::ListProviders => list_providers(proxy, ui).await,
        Command::UpsertProvider { json } => {
            send_unit(ui, proxy.upsert_provider(&json).await, "Provider saved.").await;
            list_providers(proxy, ui).await;
        }
        Command::SetSecret { id, secret } => {
            let result = proxy.set_secret(&id, &secret).await;
            drop(secret);
            send_unit(ui, result, "Key stored in the secret service.").await;
        }
        Command::ClearSecret { id } => {
            send_unit(
                ui,
                proxy.clear_secret(&id).await,
                "Key removed from the secret service.",
            )
            .await;
        }
        Command::SecretStatus { id } => secret_status(proxy, ui, &id).await,
        Command::ListModels { id } => list_models(proxy, ui, &id).await,
        Command::Discover { id } => discover_models(proxy, ui, &id).await,
        Command::StartPull { id, name } => start_pull(proxy, ui, &id, &name).await,
        Command::PullStatus => pull_status(proxy, ui).await,
        Command::Shutdown => {
            let _ = proxy.shutdown().await;
            let _ = ui.send(UiMsg::ShutdownDone).await;
        }
        Command::Confirm {
            request_id,
            call_id,
            allow,
        } => {
            let note = if allow {
                "Action allowed."
            } else {
                "Action denied."
            };
            send_unit(
                ui,
                proxy.confirm_action(&request_id, &call_id, allow).await,
                note,
            )
            .await;
        }
        Command::ListMemories => list_memories(proxy, ui).await,
        Command::ForgetMemory { id } => {
            send_unit(ui, proxy.forget_memory(&id).await, "Note deleted.").await;
            list_memories(proxy, ui).await;
        }
    }
}

async fn send_message(
    proxy: &AgentProxy<'_>,
    ui: &async_channel::Sender<UiMsg>,
    conversation_id: &str,
    provider_id: &str,
    text: &str,
) {
    let raw = match proxy.send_message(conversation_id, provider_id, text).await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::RequestFailed(err.to_string())).await;
            return;
        }
    };
    let reply: SendReply = match serde_json::from_str(&raw) {
        Ok(reply) => reply,
        Err(err) => {
            let _ = ui.send(UiMsg::RequestFailed(err.to_string())).await;
            return;
        }
    };
    let _ = ui
        .send(UiMsg::SendStarted {
            request_id: reply.request_id,
            conversation_id: reply.conversation_id,
        })
        .await;
}

async fn list_conversations(
    proxy: &AgentProxy<'_>,
    ui: &async_channel::Sender<UiMsg>,
    query: &str,
) {
    let raw = match proxy.list_conversations(query).await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<Vec<ConversationSummary>>(&raw) {
        Ok(rows) => {
            let _ = ui.send(UiMsg::Conversations(rows)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn get_messages(
    proxy: &AgentProxy<'_>,
    ui: &async_channel::Sender<UiMsg>,
    conversation_id: &str,
) {
    let raw = match proxy.get_messages(conversation_id).await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<Vec<StoredMessage>>(&raw) {
        Ok(messages) => {
            let _ = ui
                .send(UiMsg::Messages {
                    conversation_id: conversation_id.to_string(),
                    messages,
                })
                .await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn delete_conversation(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>, id: &str) {
    if let Err(err) = proxy.delete_conversation(id).await {
        let _ = ui.send(UiMsg::Status(err.to_string())).await;
        return;
    }
    let _ = ui.send(UiMsg::Deleted { id: id.to_string() }).await;
}

async fn delete_all(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>) {
    if let Err(err) = proxy.delete_all_conversations().await {
        let _ = ui.send(UiMsg::Status(err.to_string())).await;
        return;
    }
    let _ = ui.send(UiMsg::DeletedAll).await;
}

async fn list_providers(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>) {
    let raw = match proxy.list_providers().await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<Vec<ProviderConfig>>(&raw) {
        Ok(providers) => {
            let _ = ui.send(UiMsg::Providers(providers)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn list_memories(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>) {
    let raw = match proxy.list_memories().await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<Vec<MemoryNote>>(&raw) {
        Ok(notes) => {
            let _ = ui.send(UiMsg::Memories(notes)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn list_models(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>, id: &str) {
    let raw = match proxy.list_models(id).await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(models) => {
            let _ = ui.send(UiMsg::Models(models)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn discover_models(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>, id: &str) {
    let raw = match proxy.discover_models(id).await {
        Ok(raw) => raw,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
            return;
        }
    };
    match serde_json::from_str::<DiscoveryReport>(&raw) {
        Ok(report) => {
            let _ = ui.send(UiMsg::Discovery(report)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn start_pull(
    proxy: &AgentProxy<'_>,
    ui: &async_channel::Sender<UiMsg>,
    id: &str,
    name: &str,
) {
    match proxy.start_model_pull(id, name).await {
        Ok(raw) => forward_pull(ui, &raw).await,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn pull_status(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>) {
    match proxy.model_pull_status().await {
        Ok(raw) => forward_pull(ui, &raw).await,
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn forward_pull(ui: &async_channel::Sender<UiMsg>, raw: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        let _ = ui
            .send(UiMsg::Status(
                "The download status was not readable.".into(),
            ))
            .await;
        return;
    };
    let phase = value
        .get("phase")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let name = value
        .get("name")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let detail = value
        .get("detail")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let _ = ui
        .send(UiMsg::PullProgress {
            phase,
            name,
            detail,
        })
        .await;
}

async fn secret_status(proxy: &AgentProxy<'_>, ui: &async_channel::Sender<UiMsg>, id: &str) {
    match proxy.secret_status(id).await {
        Ok(status) => {
            let _ = ui.send(UiMsg::SecretStatus(status)).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn send_unit(ui: &async_channel::Sender<UiMsg>, result: zbus::Result<()>, ok: &str) {
    match result {
        Ok(()) => {
            let _ = ui.send(UiMsg::Status(ok.to_string())).await;
        }
        Err(err) => {
            let _ = ui.send(UiMsg::Status(err.to_string())).await;
        }
    }
}

async fn forward_delta(item: Option<StreamDelta>, ui: &async_channel::Sender<UiMsg>) -> bool {
    let Some(message) = item else {
        return false;
    };
    let Ok(args) = message.args() else {
        return true;
    };
    let _ = ui
        .send(UiMsg::Delta {
            request_id: args.request_id.to_string(),
            text: args.text.to_string(),
        })
        .await;
    true
}

async fn forward_event(item: Option<BusStreamEvent>, ui: &async_channel::Sender<UiMsg>) -> bool {
    let Some(message) = item else {
        return false;
    };
    let Ok(args) = message.args() else {
        return true;
    };
    let Ok(event) = serde_json::from_str::<StreamEvent>(args.event_json) else {
        return true;
    };
    let _ = ui
        .send(UiMsg::Event {
            request_id: args.request_id.to_string(),
            event,
        })
        .await;
    true
}

async fn forward_done(item: Option<StreamDone>, ui: &async_channel::Sender<UiMsg>) -> bool {
    let Some(message) = item else {
        return false;
    };
    let Ok(args) = message.args() else {
        return true;
    };
    let _ = ui
        .send(UiMsg::Done {
            request_id: args.request_id.to_string(),
            status: args.status.to_string(),
            error: args.error.to_string(),
        })
        .await;
    true
}

async fn connect(launch: &Launch) -> Result<AgentProxy<'static>, String> {
    if let Ok(proxy) = try_proxy().await {
        return Ok(proxy);
    }
    spawn_daemon(launch)?;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(proxy) = try_proxy().await {
            return Ok(proxy);
        }
    }
    Err("The daemon did not answer on the session bus.".to_string())
}

async fn try_proxy() -> zbus::Result<AgentProxy<'static>> {
    let proxy = session_proxy().await?;
    let pong = proxy.ping().await?;
    if pong != "ok" {
        return Err(zbus::Error::Failure("unexpected ping reply".into()));
    }
    Ok(proxy)
}

fn spawn_daemon(launch: &Launch) -> Result<(), String> {
    let mut process = Process::new(daemon_program());
    if let Some(config) = &launch.config {
        process.arg("--config").arg(config);
    }
    if let Some(data) = &launch.data_dir {
        process.arg("--data-dir").arg(data);
    }
    attach_log(&mut process, launch);
    let child = process
        .spawn()
        .map_err(|err| format!("could not start lda-daemon: {err}"))?;
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

fn daemon_program() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("lda-daemon");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("lda-daemon")
}

fn attach_log(process: &mut Process, launch: &Launch) {
    let dir = launch.data_dir.clone().unwrap_or_else(xdg_data_dir);
    if private_dir(&dir).is_err() {
        return;
    }
    let Ok(file) = std::fs::File::create(dir.join("daemon.log")) else {
        return;
    };
    let Ok(stderr) = file.try_clone() else {
        return;
    };
    process
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(stderr));
}
