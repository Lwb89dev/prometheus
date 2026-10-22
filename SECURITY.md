# Security

The model is an untrusted planner. Web pages and search snippets are untrusted
data. A tool runs only when Rust code accepts the name, the arguments, and the
permission. Prompt text is not authorization. Instructions inside a page, a
snippet, or a tool result are not tool calls.

## Boundaries

- The daemon is a user service. It is not installed setuid. On startup it
  reads `Uid` from `/proc/self/status` and exits if the uid is 0 or if that
  file cannot be read.
- The session bus is the trust boundary of the user session. Any process in
  that session can call `SetSecret`. That is the same trust the secret service
  already gives those processes. Secrets are not logged.
- Config files reject an `api_key` field and reject endpoint URLs that contain
  userinfo or a scheme other than `http` or `https`.
- Provider ids match `^[a-z][a-z0-9_-]{0,63}$`.
- Tool names match `^[a-z][a-z0-9_]{0,63}$`.
- User text rejects NUL and is capped at 32_000 characters.
- Tool output stored for the model is capped at 16_384 characters.
- At most 8 requests run at once. Each has a child cancellation token.
  Shutdown cancels the root token.
- Subprocesses are not started through a shell. Argument vectors are fixed.
  Package names and unit names are checked before they are passed.

## Permissions

`PermissionPolicy::default` is read-only. The daemon uses `PermissionPolicy::daemon`:
read-only tools run, user actions return `Confirm`, and system changes and
destructive tools stay denied. `ConfirmationPolicy::Deny` blocks a tool even
when the level would otherwise be allowed. Read-only plus `Always` still runs
without a dialog, so a read-only tool cannot stall the turn waiting for a click.

A user action emits `StreamEvent::Confirmation` and waits. Timeout or an
explicit deny becomes the tool error `confirmation was not granted`. It does
not invent a success. The CLI denies the action when stdin is not a terminal.

Tests cover a destructive tool that the scripted model requests, a tool fence
inside a user message, a tool fence planted inside a tool result, a user
action that is not confirmed, and a user action that is confirmed. The
unconfirmed tool does not run.

## System data

Collectors read `/proc` and `/sys`. They do not read process `environ` or
`cmdline`. DMI output is limited to product, vendor, board vendor, and BIOS
version. Serial numbers and UUIDs are not collected. Network data includes
interface addresses and MAC addresses from sysfs. DNS search domains are not
included.

`share_system_context` defaults to false in serde. The written default config
sets it true only for the local Ollama provider. A non-loopback endpoint gets
no system tools unless the flag is true. Loopback still gets them when the
flag is false, because the data stays on the machine. The settings window says
this before you enable the flag for a remote endpoint.

Battery fields are observations from the firmware interface. A missing or odd
report is not a confirmed hardware failure. Tool JSON uses `"kind": "observation"`.

## Files

`resolve_within` rejects NUL, rejects paths that escape the root, and uses
symlink-aware canonicalization. `open_directory` uses it against `HOME`.
`locate_file` walks at most five directory levels under `HOME`, skips hidden
names and symlinks, returns at most 20 paths, and does not read file contents.
`open_application` accepts a desktop id and runs `gtk-launch` with that one
argument. It does not use a shell.

SQLite and the config file are mode `0600`. The data directory is `0700`.
Config writes go to `config.toml.tmp` and then rename.

## Secrets

`KeyringSecretStore` uses the `keyring` crate (service `linux-desktop-agent`,
account = provider id). `auth_env` is the variable name. An empty secret
service entry falls through to the environment. A non-empty secret service
entry wins. If the secret service is down and the provider is local Ollama
with no `auth_env`, the daemon warns once and continues without a key.
`SetSecret` still returns the keyring error.

The keyring client is blocking. The daemon runs each secret call on a thread
named `lda-secret`, because calling it on a Tokio worker panics the runtime.
A panic inside that thread becomes an error. It does not take down the daemon.

Redaction covers `Bearer`, `sk-`, `sk-ant-`, `xai-`, and `api_key=` tokens of
at least 8 characters before a preview is logged or shown.

## Network

Provider calls use rustls. The loopback client is built with `no_proxy`.
Redirects on provider calls are limited. There is no telemetry.

Web search is off until `[web]` says otherwise. SearXNG must be a loopback
URL unless `allow_remote_search` is true. Page fetch blocks loopback, private,
link-local, CGNAT, metadata names, and similar addresses unless
`allow_private_fetch` is true. Redirects are followed manually, at most two
hops, and each hop is checked again. The body is capped at 1_000_000 bytes and
the extracted text at 12_000 characters. `script`, `style`, `noscript`,
`template`, `svg`, and `iframe` contents are dropped. The minimum interval
between web calls defaults to 2 seconds.

## Tests that must keep passing

- Permission denial, untrusted fences, and confirmation (`lda-core`).
- Path escape and symlink escape (`lda-security`).
- Plaintext `api_key` rejection (`lda-common`, `lda-daemon`).
- Schema rejection of unexpected tool arguments (`lda-tools`).
- SearXNG loopback rule, private-address rejection, and disabled search that
  does not connect (`lda-common`, `lda-web`).
- Explicit memory rejection of secret-shaped notes (`lda-memory`).
- Peer-to-peer bus error mapping (`lda-ipc`), so CI does not need a user bus.

## Not claimed

Polkit is not wired up. There is no privileged helper. The Flatpak manifest
does not grant `--filesystem=host` and has not been built. Debug logs may
contain bus payloads if you raise `RUST_LOG`.
