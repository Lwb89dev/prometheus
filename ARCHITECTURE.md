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
| `lda-llm` | Provider trait, Ollama, OpenAI-compatible, Anthropic, xAI |
| `lda-web` | Search client, HTML extraction, public-address checks |
| `lda-desktop` | Portal capabilities, notifications, OpenURI, no-voice backend |
| `lda-tools` | Allowlisted tools, JSON-schema checks, system, web, and action tools |
| `lda-core` | Agent loop: context, tool rounds, confirmation, cancellation |
| `lda-memory` | SQLite conversations, messages, tool runs, explicit notes |
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

`ConfirmAction`, `ListMemories`, `Remember`, and `ForgetMemory` are on the same
interface. `ReloadConfig` rereads the file and applies the web settings and
the `persistent_memory` flag.

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
6. Validate the name, the arguments, and the permission. Read-only tools run.
   User actions emit `Confirmation` and wait on `ConfirmHub` until
   `confirm_action` or a timeout. The daemon publishes that event and returns
   to its receive loop, so the bus method can answer without blocking the agent
   task. System changes and destructive tools are denied. Then the tool runs
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

Anthropic uses `/messages`, splits the system prompt into the top-level
`system` field, and maps `tool_use` / `input_json_delta` onto the same
`LlmDelta::ToolCall` stream. The headers are `x-api-key` and
`anthropic-version: 2023-06-01`. xAI is the OpenAI-compatible provider with
`kind = "xai"` and an endpoint you set. Neither provider is contacted unless
you configure it.

Two HTTP clients exist. The loopback client disables proxy environment
variables so `HTTP_PROXY` cannot redirect a local Ollama call.

## Memory

SQLite uses WAL, foreign keys, and a `busy_timeout`. Migrations are append-only
in `schema_migrations`. The connection is `Send` and not `Sync`. The daemon
holds it in a mutex and does not keep the guard across an await.

`persist_conversations = false` keeps the history in the daemon process.
Restart drops it. Search is a bounded SQL `LIKE` with `%`, `_`, and `\` escaped.

Passwords, tokens, and private file contents are not a stored memory type.
Notes live in a `memories` table and are written only by `remember` or
`lda-cli remember`. The system prompt includes them as data when
`persistent_memory` is true. The assistant does not infer new notes.
A note that still contains a redacted secret pattern is rejected.
Deleting a note works after the flag is turned off, from **Notes** or
`lda-cli forget`.

## Desktop

`lda-system` classifies the session from `XDG_CURRENT_DESKTOP`,
`XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, and `DISPLAY`. COSMIC is not reported as
GNOME. Compositor names (`cosmic-comp`, `mutter`, `kwin`, `xfwm4`) are hints.
`lda-desktop` reads the portal introspection and reports notification,
OpenURI, GlobalShortcuts, and whether `gtk4-layer-shell-0` is installed.
It calls `AddNotification` and `OpenURI` only when a tool or a confirmation
asks it to. It does not register a shortcut. `tested_here` is true only for
COSMIC and GNOME names; a KDE or XFCE classification says that desktop was
not exercised here.

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

`VoiceBackend` is the trait. The only implementation is `NoVoice`, whose
`available` is false. The GTK window does not open a microphone and does not
show a control that claims to listen. A later local backend can implement the
trait with push-to-talk. Do not enable an always-on microphone by default.

## Logging

The default filter is `info,zbus=warn`. Info lines include provider id,
conversation id, tool name, and ok/err. They do not include message bodies,
tool arguments, or secrets. `RUST_LOG=debug` can include bus payloads. Treat
that as sensitive.
