//! Talks to a running daemon for chat and history.
//! System snapshots and read-only tools also run in this process, so they work
//! when the daemon is not up. They use the same allowlist and permission check.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use lda_common::{Error, Result, SendReply, StreamEvent};
use lda_ipc::{session_proxy, AgentProxy, BusStreamEvent, StreamDelta, StreamDone};
use lda_security::PermissionPolicy;
use lda_tools::{prepare_call, system_registry};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "lda-cli", version, about = "Linux Desktop Agent command line")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check that the daemon answers on the session bus.
    Status,
    /// Send one message and print the streamed reply.
    Chat {
        #[arg(short, long)]
        message: String,
        /// Existing conversation id. Empty starts a new one.
        #[arg(short, long, default_value = "")]
        conversation: String,
        /// Provider id. Empty uses the first configured provider.
        #[arg(short, long, default_value = "")]
        provider: String,
    },
    /// List stored conversations.
    Conversations {
        #[arg(short, long, default_value = "")]
        query: String,
    },
    /// Print one conversation.
    Show { id: String },
    /// Delete one conversation.
    Delete { id: String },
    /// Delete every stored conversation.
    DeleteAll {
        #[arg(long)]
        yes: bool,
    },
    /// Write stored conversations as JSON.
    Export {
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Print a local system snapshot. Does not contact a model.
    Sysinfo,
    /// List read-only tools.
    Tools,
    /// Run one read-only tool in this process.
    Tool {
        name: String,
        #[arg(long, default_value = "{}")]
        args: String,
    },
    /// Cancel a running request.
    Cancel { request_id: String },
    /// Ask the daemon to shut down.
    Shutdown,
    /// List models advertised by a provider.
    Models {
        #[arg(short, long, default_value = "")]
        provider: String,
    },
    /// List notes the user explicitly saved.
    Memories,
    /// Store one note. Persistent memory must be enabled in the configuration.
    Remember { text: String },
    /// Delete one saved note by id.
    Forget { id: String },
    /// Reread the configuration file.
    Reload,
}

#[tokio::main]
async fn main() {
    if let Err(err) = dispatch(Cli::parse()).await {
        eprintln!("lda-cli: {err}");
        std::process::exit(1);
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Status => status().await,
        Commands::Chat {
            message,
            conversation,
            provider,
        } => chat(&message, &conversation, &provider).await,
        Commands::Conversations { query } => print_json(
            &proxy()
                .await?
                .list_conversations(&query)
                .await
                .map_err(ipc)?,
        ),
        Commands::Show { id } => {
            print_messages(&proxy().await?.get_messages(&id).await.map_err(ipc)?)
        }
        Commands::Delete { id } => proxy().await?.delete_conversation(&id).await.map_err(ipc),
        Commands::DeleteAll { yes } => delete_all(yes).await,
        Commands::Export { out } => export(out).await,
        Commands::Sysinfo => print_value(&lda_system::system_info()),
        Commands::Tools => print_tools(),
        Commands::Tool { name, args } => call_tool(&name, &args).await,
        Commands::Cancel { request_id } => proxy().await?.cancel(&request_id).await.map_err(ipc),
        Commands::Shutdown => proxy().await?.shutdown().await.map_err(ipc),
        Commands::Models { provider } => {
            print_json(&proxy().await?.list_models(&provider).await.map_err(ipc)?)
        }
        Commands::Memories => print_json(&proxy().await?.list_memories().await.map_err(ipc)?),
        Commands::Remember { text } => {
            print_json(&proxy().await?.remember(&text).await.map_err(ipc)?)
        }
        Commands::Forget { id } => proxy().await?.forget_memory(&id).await.map_err(ipc),
        Commands::Reload => proxy().await?.reload_config().await.map_err(ipc),
    }
}

async fn proxy() -> Result<AgentProxy<'static>> {
    session_proxy().await.map_err(|err| {
        Error::Ipc(format!(
            "{err}. Start lda-daemon, or start lda-gtk once so it can launch the daemon."
        ))
    })
}

async fn status() -> Result<()> {
    let proxy = session_proxy()
        .await
        .map_err(|err| Error::Ipc(format!("daemon is not reachable: {err}")))?;
    let pong = proxy.ping().await.map_err(ipc)?;
    println!("{pong}");
    Ok(())
}

async fn chat(message: &str, conversation: &str, provider: &str) -> Result<()> {
    let proxy = proxy().await?;
    let mut deltas = proxy.receive_stream_delta().await.map_err(ipc)?;
    let mut events = proxy.receive_stream_event().await.map_err(ipc)?;
    let mut dones = proxy.receive_stream_done().await.map_err(ipc)?;
    let raw = proxy
        .send_message(conversation, provider, message)
        .await
        .map_err(ipc)?;
    let reply: SendReply = serde_json::from_str(&raw).map_err(|err| Error::Ipc(err.to_string()))?;
    eprintln!(
        "request {} conversation {}",
        reply.request_id, reply.conversation_id
    );
    loop {
        let finished = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                proxy.cancel(&reply.request_id).await.map_err(ipc)?;
                false
            }
            item = deltas.next() => {
                show_delta(item, &reply.request_id)?;
                false
            }
            item = events.next() => {
                show_event(item, &reply.request_id, &proxy).await?;
                false
            }
            item = dones.next() => show_done(item, &reply.request_id)?,
        };
        if finished {
            return Ok(());
        }
    }
}

fn show_delta(item: Option<StreamDelta>, request_id: &str) -> Result<()> {
    let Some(message) = item else {
        return Err(Error::Ipc("delta stream ended".into()));
    };
    let args = message.args().map_err(ipc)?;
    if args.request_id != request_id {
        return Ok(());
    }
    print!("{}", args.text);
    io::stdout().flush()?;
    Ok(())
}

async fn show_event(
    item: Option<BusStreamEvent>,
    request_id: &str,
    proxy: &AgentProxy<'_>,
) -> Result<()> {
    let Some(message) = item else {
        return Err(Error::Ipc("event stream ended".into()));
    };
    let args = message.args().map_err(ipc)?;
    if args.request_id != request_id {
        return Ok(());
    }
    let event: StreamEvent =
        serde_json::from_str(args.event_json).map_err(|err| Error::Ipc(err.to_string()))?;
    match event {
        StreamEvent::ToolStarted { name, .. } => eprintln!("\ntool {name} started"),
        StreamEvent::ToolFinished { name, ok, .. } => eprintln!("\ntool {name} finished ok={ok}"),
        StreamEvent::ReplaceText { text } => {
            println!("\n{text}");
            io::stdout().flush()?;
        }
        StreamEvent::Sources { items } => {
            eprintln!("\nsources:");
            for item in items {
                eprintln!("- {} ({})", item.title, item.url);
            }
        }
        StreamEvent::Confirmation {
            call_id,
            name,
            summary,
        } => {
            eprintln!("\nconfirm {name}: {summary}");
            let allow = confirm_on_tty().await;
            proxy
                .confirm_action(request_id, &call_id, allow)
                .await
                .map_err(ipc)?;
            if allow {
                eprintln!("allowed");
            } else {
                eprintln!("denied");
            }
        }
    }
    Ok(())
}

async fn confirm_on_tty() -> bool {
    let joined = tokio::task::spawn_blocking(read_yes).await;
    joined.unwrap_or(false)
}

fn read_yes() -> bool {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        eprintln!("not a terminal; denying the action");
        return false;
    }
    eprint!("allow? [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes")
}

fn show_done(item: Option<StreamDone>, request_id: &str) -> Result<bool> {
    let Some(message) = item else {
        return Err(Error::Ipc("done stream ended".into()));
    };
    let args = message.args().map_err(ipc)?;
    if args.request_id != request_id {
        return Ok(false);
    }
    println!();
    if args.status == "ok" || args.status == "cancelled" {
        if args.status == "cancelled" {
            eprintln!("cancelled");
        }
        return Ok(true);
    }
    let detail = if args.error.is_empty() {
        args.status.to_string()
    } else {
        args.error.to_string()
    };
    Err(Error::Ipc(detail))
}

fn ipc(err: zbus::Error) -> Error {
    Error::Ipc(err.to_string())
}

fn print_json(raw: &str) -> Result<()> {
    let value: Value = serde_json::from_str(raw).map_err(|err| Error::Ipc(err.to_string()))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string())
    );
    Ok(())
}

fn print_value(value: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|err| Error::Other(err.to_string()))?
    );
    Ok(())
}

fn print_messages(raw: &str) -> Result<()> {
    let messages: Vec<lda_common::StoredMessage> =
        serde_json::from_str(raw).map_err(|err| Error::Ipc(err.to_string()))?;
    for message in messages {
        println!("--- {} ---", message.role.as_str());
        println!("{}", message.content);
    }
    Ok(())
}

async fn delete_all(yes: bool) -> Result<()> {
    if !yes {
        return Err(Error::InvalidInput(
            "pass --yes to delete every conversation".into(),
        ));
    }
    proxy().await?.delete_all_conversations().await.map_err(ipc)
}

async fn export(out: Option<PathBuf>) -> Result<()> {
    let raw = proxy().await?.export_data().await.map_err(ipc)?;
    let value: Value = serde_json::from_str(&raw).map_err(|err| Error::Ipc(err.to_string()))?;
    let pretty =
        serde_json::to_string_pretty(&value).map_err(|err| Error::Other(err.to_string()))?;
    let Some(path) = out else {
        println!("{pretty}");
        return Ok(());
    };
    std::fs::write(&path, pretty)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn print_tools() -> Result<()> {
    for tool in system_registry().definitions() {
        println!("{}\t{}", tool.name, tool.description);
    }
    Ok(())
}

async fn call_tool(name: &str, args: &str) -> Result<()> {
    let registry = system_registry();
    let Some(tool) = registry.get(name) else {
        return Err(Error::NotFound(name.to_string()));
    };
    let value: Value =
        serde_json::from_str(args).map_err(|err| Error::InvalidInput(err.to_string()))?;
    let spec = tool.spec();
    if !PermissionPolicy::default().allows(spec.permission, spec.confirmation) {
        return Err(Error::PermissionDenied(format!(
            "{} is not permitted",
            spec.name
        )));
    }
    prepare_call(tool.as_ref(), &value)?;
    let result = tool.execute(&value, &CancellationToken::new()).await?;
    print_value(&result)
}
