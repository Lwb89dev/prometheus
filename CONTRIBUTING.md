# Contributing

Work in small changes that still build and pass tests. Do not add an empty
crate for a later milestone. Do not commit secrets, conversation databases, or
`config.toml.tmp`.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The project forbids `unsafe` in its own crates. Keep that.

## Add a tool

1. Implement the collection in `lda-system` if it reads the machine. Prefer
   `/proc`, `/sys`, or D-Bus over parsing human-oriented command output.
2. Add a `ToolSpec` in `crates/lda-tools/src/system.rs` through `system_registry`,
   or register another `Tool` on the same registry.
3. Set the permission, risk, timeout, and confirmation policy. Read-only tools
   use `PermissionLevel::ReadOnly`, `Risk::Low`, and `ConfirmationPolicy::Never`.
4. Give the parameters a JSON schema. `validate_arguments` checks a subset:
   object shape, required fields, types, and simple numeric bounds.
5. Cap the output. Do not include secrets, process environments, or command lines.
6. Add a test that a bad argument is rejected and that a destructive level is
   denied by `PermissionPolicy` even if the tool is registered.
7. Do not start a shell. If you must run a program, pass an argument vector
   and a timeout, and validate every interpolated field first.

The agent loop will refuse the tool when the policy says no. You do not need
to change `lda-core` for a new read-only tool.

## Add an LLM provider

1. Implement `LlmProvider` in a new module under `crates/lda-llm`.
2. Report capabilities honestly. If the provider cannot call tools, set
   `tools: false`. The loop then uses the text fence only when
   `tool_calling` is `auto` or `text`.
3. Map the provider's stream into `LlmDelta`. Do not invent tool results.
4. Extend `ProviderKind` and `build_provider`. Keep endpoint checks in
   `lda-common`: http(s) only, no userinfo.
5. Take the API key as an argument. Do not read it from the config file.
6. Add parser tests with fixture bytes. Do not call the network from unit tests.
7. Leave `lda-core` on the `LlmProvider` trait. The loop should not gain a
   match on the vendor.

## Add a desktop adapter

Do this when you can test the desktop, not before.

1. Introduce `DesktopEnvironmentAdapter` in a crate that the daemon or the GTK
   app can select at runtime.
2. Detect capabilities. A method that the session cannot do must return a
   clear unsupported error. Do not assume an X11 feature exists on Wayland.
3. Global shortcuts go through a portal or a documented desktop mechanism.
   Do not override an existing shortcut. If none is available, document the
   fallback (launch `lda-gtk` from the menu or a terminal).
4. A floating widget that needs layer-shell or an equivalent should live in
   the adapter. The current `EntityWidget` is only the drawing and the state.
5. Write down what you ran and what you did not run in `DEVELOPMENT.md`.
   Untested integration is not a supported feature.

## Secrets and data

Do not put a real key in a test, a log, or a commit. Use `MemorySecretStore`
in tests. Live checks belong in a temporary `--data-dir`, not in your real
`~/.local/share/lda`, unless you mean to use that database.
