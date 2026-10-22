# Security

The model is an untrusted planner. Web pages are not fetched in this milestone.
When they are, they will be untrusted data too. A tool runs only when Rust
code accepts the name, the arguments, and the permission. Prompt text is not
authorization.

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

`PermissionPolicy` defaults to read-only. `USER_ACTION`, `SYSTEM_MODIFICATION`,
and `DESTRUCTIVE` are denied even if a tool is registered. `ConfirmationPolicy::Deny`
blocks a tool even when the level would otherwise be allowed. There is no
confirmation dialog for mutating tools yet, so those levels stay off.

Tests cover a destructive tool that the scripted model requests, a tool fence
inside a user message, and a tool fence planted inside a tool result. None of
those run a second tool.

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

`resolve_within` is the helper for a future file tool. It rejects NUL, rejects
paths that escape the root, and uses symlink-aware canonicalization. No file
tool is registered in this milestone, so the helper is tested and unused by
the agent.

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
Redirects are limited. There is no telemetry and no call that is not a
provider request, a model list, or a system command the tool itself runs.

## Tests that must keep passing

- Permission denial and untrusted fences (`lda-core`).
- Path escape and symlink escape (`lda-security`).
- Plaintext `api_key` rejection (`lda-common`, `lda-daemon`).
- Schema rejection of unexpected tool arguments (`lda-tools`).
- Peer-to-peer bus error mapping (`lda-ipc`), so CI does not need a user bus.

## Not claimed

Polkit is not wired up. There is no privileged helper. Flatpak sandboxing is
not implemented; a later Flatpak must not silently punch holes for diagnostics.
Debug logs may contain bus payloads if you raise `RUST_LOG`.
