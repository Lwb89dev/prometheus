# Development

## Environment used for this tree

Checked on the machine where Milestone 0 and 1 were implemented:

- Pop!_OS 24.04 LTS (noble), x86_64, kernel from the running system
- Session: COSMIC on Wayland (`XDG_CURRENT_DESKTOP=COSMIC`, `XDG_SESSION_TYPE=wayland`)
- Rust stable via rustup. `rust-toolchain.toml` asks for rustfmt and clippy.
  The first Cargo command may install those components under `~/.rustup`.
- GTK 4.14 development files were present. `libadwaita-1-dev` was not, so the
  UI does not use libadwaita.
- SQLite and D-Bus headers were present. `rusqlite` links the system library.
- Ollama was installed and listed `mistral:latest` with a `tools` capability.
- The secret service name `org.freedesktop.secrets` was on the session bus.
- The user systemd instance was degraded. The daemon was started by hand.
  The systemd unit was not enabled and was not verified.

Do not treat the compositor hint as proof that a COSMIC integration works.
Detection tests only check the classification function.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

GTK unit tests must not call `gtk::init`. The markdown test only checks span
extraction. CI is `.github/workflows/ci.yml` on Ubuntu 24.04 with
`libgtk-4-dev`, `pkg-config`, and `libsqlite3-dev`. It does not install
libadwaita. The IPC test uses a peer-to-peer socket and does not need a session bus.

## Live check without touching the real config

```bash
dir=$(mktemp -d /tmp/lda-dev-XXXX)
cargo build -p lda-daemon -p lda-cli -p lda-gtk
./target/debug/lda-daemon --config "$dir/config.toml" --data-dir "$dir"
./target/debug/lda-cli chat -m "Use get_kernel_info and quote the release."
./target/debug/lda-cli shutdown
rm -rf "$dir"
```

If `org.linuxdesktopagent.Daemon` is already owned, the new daemon exits 0 and
the CLI talks to the existing one. Do not kill a daemon you did not start.

Compare `get_kernel_info` with `/proc/sys/kernel/osrelease`. A model that emits
a `tool` fence instead of a native tool call still satisfies the loop when
`tool_calling` is `auto`. If native calls fail, set `tool_calling = "text"`.

## Libraries pinned on purpose

- `zbus` is exactly 5.19. zbus 6 changed the connection builder. Stay on 5
  until a deliberate port.
- `gtk4` 0.11 with feature `v4_14` only. Do not enable `v4_16` on a 4.14 system.
- `reqwest` uses rustls, not OpenSSL.
- `rusqlite` uses the system SQLite. If that fails to link, switch that crate
  to the bundled feature.

## Indentation

Keep functions shallow. More than three indentation levels is a signal to
return early or extract a function, unless the extra level is actually required.

## Not verified here

- KDE Plasma, XFCE, and X11 sessions.
- Global shortcuts and layer-shell.
- A remote OpenAI-compatible provider with a real key.
- Install of the systemd user unit, the D-Bus service file, or the desktop entry.
- libadwaita.
- Performance numbers. None are claimed.
