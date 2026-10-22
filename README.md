# Linux Desktop Agent

Linux Desktop Agent (LDA) is a local personal assistant for Linux desktops.
A user-level daemon talks to a local or OpenAI-compatible model. A GTK 4 window
streams the reply, and read-only system tools can answer questions about the
machine you are actually running.

The name, icon, and colors are ordinary files. Replace them without touching
the agent.

This repository is GPL-3.0-or-later. See [LICENSE](LICENSE).

## What works in this milestone

- `lda-daemon` runs as the current user. It does not need root or systemd.
- `lda-gtk` connects over the session bus and can start the daemon if it is down.
- Closing the window leaves the daemon running. **Quit daemon** asks it to stop.
- Ollama and any OpenAI-compatible HTTP endpoint.
- Streaming replies, cancellation, and local SQLite history.
- The model can call registered read-only system tools. Results go back to the model.
- The Prometeus entity from `lda-visual` opens with the conversation window.
  Connection, streaming, and tool events drive its animation. **Widget** hides
  or shows that window. It is a normal GTK window, not a layer-shell overlay.
- API keys go to the desktop secret service, or to an environment variable you name.
  They are not written into `config.toml`.

## What is not in this milestone

- No global shortcut. Super+Space is not registered.
- The optional widget is a normal GTK window, not a layer-shell overlay.
- No COSMIC, GNOME, KDE, or XFCE adapter beyond desktop detection.
- No libadwaita. The UI is GTK 4.
- No web search, no package install or removal, no service start or stop, no journal tool.
- No voice capture. The visual layer can show a microphone level if a future
  capture pipeline supplies one. It does not open a microphone.
- No `.deb`, RPM, or Flatpak build. Unit and desktop files are in `packaging/`
  and are not installed for you.

On this machine, `mistral:latest` did not emit native Ollama tool calls. A JSON
fence still ran `get_kernel_info`, and the message after that result quoted the
kernel from `/proc`. The same first message also contained a guessed kernel
string before the tool returned. That guess is model prose, not a tool result.

`lda-cli shutdown` stops the process. zbus `graceful_shutdown` did not return
within 2 seconds; the daemon logs `bus shutdown timed out` and then `stopped`.

See [ROADMAP.md](ROADMAP.md) and [SECURITY.md](SECURITY.md).

## Build

You need Rust stable (1.85 or newer; gtk4 0.11 wants a current stable),
`pkg-config`, GTK 4.14 development files, and SQLite development files.

```bash
sudo apt install libgtk-4-dev pkg-config libsqlite3-dev
cargo build --workspace
cargo test --workspace
```

Binaries:

- `target/debug/lda-daemon`
- `target/debug/lda-gtk`
- `target/debug/lda-cli`

## Run

Start the daemon yourself, or let the window start it. The first launch writes
a default config that points at Ollama on `127.0.0.1:11434` with model
`mistral:latest`.

```bash
cargo run -p lda-daemon
cargo run -p lda-gtk
```

Defaults:

- Config: `$XDG_CONFIG_HOME/lda/config.toml` (usually `~/.config/lda/config.toml`)
- Database: `$XDG_DATA_HOME/lda/lda.db` (usually `~/.local/share/lda/lda.db`)

Both files are created with mode `0600`. Directories are `0700`.

Override them when you do not want to touch those paths:

```bash
cargo run -p lda-daemon -- --config /tmp/lda/config.toml --data-dir /tmp/lda
cargo run -p lda-gtk -- --config /tmp/lda/config.toml --data-dir /tmp/lda
```

`lda-gtk` forwards those flags only when it spawns the daemon. If a daemon is
already on the bus, the flags are ignored and the window attaches to it.

The command line does not spawn a daemon:

```bash
lda-cli status
lda-cli chat -m "Call get_kernel_info and quote the kernel release."
lda-cli sysinfo
lda-cli tools
lda-cli tool get_memory_info
lda-cli shutdown
```

`sysinfo` and `tool` run in the CLI process. They still go through the same
allowlist and permission check. Chat, history, and shutdown need the daemon.

In the window, Enter sends and Shift+Enter inserts a newline. Stop cancels the
current request.

## Providers

Edit the provider in the window (**Settings**) or in `config.toml`. The daemon
is the only writer of that file. A field named `api_key` is rejected.

```toml
[[providers]]
id = "ollama"
kind = "ollama"
endpoint = "http://127.0.0.1:11434"
model = "mistral:latest"
temperature = 0.2
timeout_secs = 180
context_messages = 20
context_window = 8192
share_system_context = true
tool_calling = "auto"
```

`kind` is `ollama` or `openai_compatible`. `tool_calling` is `auto`, `native`,
`text`, or `off`.

For a cloud endpoint, set `auth_env` to the **name** of an environment variable,
or use **Save key** in the window. The secret service account is the provider
id, and the service name is `linux-desktop-agent`. If the secret service has a
value, it wins over the environment variable.

Loopback providers can use system tools even when `share_system_context` is
false. Any other endpoint receives system tools only when that flag is true.
Tool results for a remote model leave the computer.

## Permissions

Tools declare a level: `READ_ONLY`, `USER_ACTION`, `SYSTEM_MODIFICATION`, or
`DESTRUCTIVE`. This milestone allows `READ_ONLY` only. The check is Rust code
in the agent loop, not a sentence in the prompt. See [SECURITY.md](SECURITY.md).

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

[DEVELOPMENT.md](DEVELOPMENT.md) covers the environment this tree was built on.
[ARCHITECTURE.md](ARCHITECTURE.md) describes the crates.
[CONTRIBUTING.md](CONTRIBUTING.md) explains how to add a tool, a provider, or,
later, a desktop adapter.

## Uninstall

Stop the daemon from the window or with `lda-cli shutdown`. Remove
`~/.config/lda` and `~/.local/share/lda` if you want the config and history gone.
If you copied the files in `packaging/`, remove those copies as well. Nothing in
this repository enables a systemd unit for you.
