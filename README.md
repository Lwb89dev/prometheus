# Linux Desktop Agent

Linux Desktop Agent (LDA) is a local personal assistant for Linux desktops.
A user-level daemon talks to a local or OpenAI-compatible model. A GTK 4 and
libadwaita window streams the reply, and read-only system tools can answer
questions about the machine you are actually running. The animated mark stays
a separate GTK 4 widget. The daemon does not link libadwaita.

The name, icon, and colors are ordinary files. Replace them without touching
the agent.

This repository is GPL-3.0-or-later. See [LICENSE](LICENSE).

## What works in this tree

Version 0.2.0. Milestone 0 and 1 behavior is still there, plus the pieces below.

- `lda-daemon` runs as the current user. It does not need root or systemd.
- `lda-gtk` connects over the session bus and can start the daemon if it is down.
- Closing the window leaves the daemon running. **Quit daemon** asks it to stop.
- Ollama, OpenAI-compatible HTTP, Anthropic Messages, and xAI (OpenAI-compatible
  against the endpoint you set, for example `https://api.x.ai/v1`).
- Streaming replies, cancellation, and local SQLite history.
- Read-only system tools. Results go back to the model as untrusted data.
- Optional web search and page fetch. Both stay off until `[web]` is enabled.
  Search snippets and page text are labeled, and http(s) links show in a source
  list separate from the reply.
- User actions (`open_uri`, `open_directory`, `open_application`, `remember`,
  `forget_memory`) wait for Allow in the window, or `y` on a terminal
  `lda-cli chat`. A non-terminal CLI denies them. Read-only tools do not ask.
- Optional notes in SQLite. They are off until `persistent_memory` is true, and
  the assistant does not invent them.
- Notifications use the notification portal when that interface is on the bus.
- The Prometeus entity from `lda-visual` opens with the conversation window.
  **Widget** hides or shows that window. It is a normal GTK window.
- API keys go to the desktop secret service, or to an environment variable you name.
  They are not written into `config.toml`.

## What this tree does not do

- No global shortcut. Super+Space is not registered. On the COSMIC session used
  here, the GlobalShortcuts portal was not in the desktop portal introspection.
- No layer-shell overlay. `gtk4-layer-shell-0` was not installed, so the mark
  stays in a normal window.
- KDE Plasma and XFCE were not run. COSMIC is not treated as GNOME.
  libadwaita is used by the GTK client only.
- No package install or removal, no service start or stop, no journal tool,
  and no polkit helper.
- **Talk** is push-to-talk. The microphone opens only while Talk is on, and
  closes on the next click, after 30 seconds, or when the window closes.
  There is no wake word. A conversation model takes typed chat. A
  speech-to-text model transcribes. A speech model is read aloud and is not
  sent a chat. Microphone audio stays on a loopback server. `NoVoice` remains
  an off switch that does not spawn a recorder.
- Web search has no default public endpoint. Page fetch refuses loopback,
  private, link-local, and metadata addresses unless `allow_private_fetch` is
  set. That flag is for a test you choose, not a default.
- The `.deb` recipe is `packaging/deb/build.sh`. The RPM spec and the Flatpak
  manifest are in `packaging/` and were not built here (`rpmbuild` and
  `flatpak-builder` are not installed). Nothing in this repository enables a
  systemd unit.

On this machine, `mistral:latest` did not emit native Ollama tool calls. A JSON
fence still ran `get_kernel_info`, and the message after that result quoted the
kernel from `/proc`. The same first message also contained a guessed kernel
string before the tool returned. That guess is model prose, not a tool result.

`lda-cli shutdown` stops the process. zbus `graceful_shutdown` did not return
within 2 seconds; the daemon logs `bus shutdown timed out` and then `stopped`.

See [ROADMAP.md](ROADMAP.md) and [SECURITY.md](SECURITY.md).

## Build

You need Rust stable (1.85 or newer; gtk4 0.11 wants a current stable),
`pkg-config`, GTK 4.14 development files, libadwaita 1.5 development files,
and SQLite development files.

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config libsqlite3-dev
cargo build --workspace
cargo test --workspace
```

Binaries:

- `target/debug/lda-daemon`
- `target/debug/lda-gtk`
- `target/debug/lda-cli`

## Run

Start the daemon yourself, or let the window start it. The first launch writes
a default config that points at Ollama on `127.0.0.1:11434` and leaves `model`
empty. The window lists installed models and does not download one until you
confirm a suggested tag. Open **Models** again later to change the saved name.

From a desktop session, `./run.sh` builds the three debug binaries, stops a
daemon that already owns the session bus, starts `ollama serve` when the
`ollama` command exists but is not answering, and opens the window. Arguments
after the script name are passed to `lda-gtk`.

```bash
./run.sh
```

The same programs can be started by hand:

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
lda-cli memories
lda-cli reload
lda-cli shutdown
```

`sysinfo` and `tool` run in the CLI process with the read-only system registry.
Chat, history, memories, reload, and shutdown need the daemon.

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
model = "qwen2.5:7b"
temperature = 0.2
timeout_secs = 180
context_messages = 20
context_window = 8192
share_system_context = true
tool_calling = "auto"
```

The sample `model` above is a name you chose. A generated file uses `model = ""`
until onboarding saves one. `lda-cli discover` prints installed models and
suggestions. `lda-cli pull qwen2.5:7b` downloads that catalog tag through
loopback Ollama after you run the command.

`kind` is `ollama`, `openai_compatible`, `anthropic`, or `xai`. Anthropic uses
the Messages API (`x-api-key` and `anthropic-version`), not a bearer token.
xAI uses the OpenAI-compatible client and the endpoint you configure.
`tool_calling` is `auto`, `native`, `text`, or `off`.

Web search is a separate table. Leave it disabled, or point it at a SearXNG
you run yourself:

```toml
[agent]
persistent_memory = false

[web]
enabled = false
kind = "none"
endpoint = ""
max_results = 5
timeout_secs = 20
min_interval_ms = 2000
allow_remote_search = false
allow_private_fetch = false
```

`kind` may be `searxng` or `json`. A SearXNG endpoint must be loopback unless
`allow_remote_search` is true. After you edit the file, run `lda-cli reload`.
The window does not write `[web]` itself. `remember` fails while
`persistent_memory` is false. **Notes** lists saved notes and can delete one
even when new notes are off. `lda-cli memories` and `lda-cli forget` do the
same from the command line.

For a cloud endpoint, set `auth_env` to the **name** of an environment variable,
or use **Save key** in the window. The secret service account is the provider
id, and the service name is `linux-desktop-agent`. If the secret service has a
value, it wins over the environment variable.

Loopback providers can use system tools even when `share_system_context` is
false. Any other endpoint receives system tools only when that flag is true.
Tool results for a remote model leave the computer.

## Permissions

Tools declare a level: `READ_ONLY`, `USER_ACTION`, `SYSTEM_MODIFICATION`, or
`DESTRUCTIVE`. The daemon allows read-only tools and asks before a user action.
System changes and destructive tools stay denied. There is no polkit helper.
The check is Rust code in the agent loop, not a sentence in the prompt.
See [SECURITY.md](SECURITY.md).

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

[DEVELOPMENT.md](DEVELOPMENT.md) covers the environment this tree was built on.
[ARCHITECTURE.md](ARCHITECTURE.md) describes the crates.
[CONTRIBUTING.md](CONTRIBUTING.md) explains how to add a tool, a provider, or
a desktop capability.

## Uninstall

Stop the daemon from the window or with `lda-cli shutdown`. Remove
`~/.config/lda` and `~/.local/share/lda` if you want the config and history gone.
If you copied the files in `packaging/`, remove those copies as well. Nothing in
this repository enables a systemd unit for you.
