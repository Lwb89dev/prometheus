# Architecture

Linux Desktop Agent is a Cargo workspace. The daemon and the GTK window are
separate processes. The daemon is useful without the window. The window does
not own the daemon's lifetime.

## Crates

| Crate | Responsibility |
| --- | --- |
| `lda-common` | Config, messages, errors, paths, permission enums, bus names |
| `lda-security` | Permission policy, path confinement, redaction, secrets |
| `lda-system` | Read-only collectors over `/proc`, `/sys`, and a few fixed commands |
| `lda-llm` | Provider trait, Ollama, OpenAI-compatible streaming |
| `lda-tools` | Allowlisted tools, JSON-schema checks, system tool registry |
| `lda-core` | Agent loop: context, tool rounds, cancellation, iteration cap |
| `lda-memory` | SQLite conversations, messages, tool runs |
| `lda-ipc` | Session-bus API and a peer-to-peer test transport |
| `lda-visual` | State model and Cairo drawing for the optional mark |
| `lda-daemon` | Process that serves the bus |
| `lda-gtk` | Conversation window |
| `lda-cli` | Status, chat, history, and in-process read-only tools |

`lda-tools` depends on `lda-llm` for `ToolDefinition`. The provider trait does
not depend on the tool registry, so adding a provider does not require a new
tool, and the reverse is also true.

## Bus

Service `org.linuxdesktopagent.Daemon`, path `/org/linuxdesktopagent/Agent`,
interface `org.linuxdesktopagent.Agent1`.

Methods take and return strings. Structured values are JSON. Signals:

- `StreamDelta(request_id, text)`
- `StreamEvent(request_id, event_json)`
- `StreamDone(request_id, status, error)`

`status` is `ok`, `cancelled`, or `error`. The client subscribes before
`SendMessage` returns and reloads the conversation when `StreamDone` arrives,
so a missed delta is reconciled from SQLite.

A second daemon sees `NameTaken` and exits 0. That lets the window spawn a
daemon without caring whether one is already running.

The core libraries do not link to systemd. The unit file in `packaging/` is
optional.

## Agent loop

1. Rebuild the system prompt. It is not stored.
2. Keep the last `context_messages` and a character budget of `context_window * 3`.
3. Offer tools only when the policy and the data-sharing rule allow them.
4. Stream a provider round.
5. Accept native tool calls. If there are none, accept one fenced `tool` block
   from the assistant text, or a single fenced JSON object whose only keys are
   `name` and optional `arguments`. Two JSON fences are ignored. User text and
   tool results are not parsed for calls.
6. Validate the name, the arguments, and the permission. Then run the tool
   with a timeout and a cancellation token.
7. Persist the assistant tool-call message and the tool result. Send the result
   back as untrusted data.
8. Stop after `max_tool_iterations` (default 4, range 1–16) and make one more
   provider call with tools disabled. That instruction is a system message in
   memory only.

The model never receives a shell. Commands that do run (`systemctl show`,
`dpkg-query`, `rpm`, `ip -json addr`) use `exec` with a fixed argument vector,
a timeout, and a byte cap.

## Providers

`LlmProvider` exposes id, capabilities, `list_models`, and `stream_chat`.
Capabilities are explicit: streaming, tools, temperature, usage. Ollama tool
calls use its native `tools` field. OpenAI-compatible calls use
`/v1/chat/completions` and SSE. A base URL that already ends in
`chat/completions` is not joined twice.

`tool_calling = "off"` offers nothing. `"native"` does not use the text fence.
`"text"` does not send a tools array. `"auto"` does both.

Two HTTP clients exist. The loopback client disables proxy environment
variables so `HTTP_PROXY` cannot redirect a local Ollama call.

## Memory

SQLite uses WAL, foreign keys, and a `busy_timeout`. Migrations are append-only
in `schema_migrations`. The connection is `Send` and not `Sync`. The daemon
holds it in a mutex and does not keep the guard across an await.

`persist_conversations = false` keeps the history in the daemon process.
Restart drops it. Search is a bounded SQL `LIKE` with `%`, `_`, and `\` escaped.

Passwords, tokens, and private file contents are not a stored memory type.
The milestone does not infer memories from the chat.

## Desktop

`lda-system` classifies the session from `XDG_CURRENT_DESKTOP`,
`XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, and `DISPLAY`. COSMIC is not reported as
GNOME. Compositor names (`cosmic-comp`, `mutter`, `kwin`, `xfwm4`) are hints.
There is no `DesktopEnvironmentAdapter` yet, because no adapter has a real
implementation. Add the trait with the first adapter that registers a shortcut
or a portal feature, and test it on that desktop before documenting it as working.

`lda-visual` draws a procedural mark from a state machine. The GTK host creates
one `EntityWidget` and forwards events that have already happened: connection,
request accepted, stream deltas, tool events, cancellation, and shutdown.
The widget does not invent those events. Voice reactivity is off unless the
host sets `voice_reactivity` and feeds a level. The crate does not open a
microphone.

On Wayland the host cannot promise a global coordinate for that window. The
saved placement is a corner plus a normalized offset so a later adapter can
clamp it to the work area. This milestone shows the mark in a normal GTK window.

## Voice

Not implemented. A later backend (whisper.cpp, Vosk, Piper, or another local
engine) should sit behind a trait with push-to-talk, a chosen language, and
local processing. Do not enable an always-on microphone by default. Wake word
stays optional and off.

## Logging

The default filter is `info,zbus=warn`. Info lines include provider id,
conversation id, tool name, and ok/err. They do not include message bodies,
tool arguments, or secrets. `RUST_LOG=debug` can include bus payloads. Treat
that as sensitive.
