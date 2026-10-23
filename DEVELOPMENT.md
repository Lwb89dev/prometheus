# Development

## Environment used for this tree

Checked on the machine where Milestone 0 and 1 were implemented:

- Pop!_OS 24.04 LTS (noble), x86_64, kernel from the running system
- Session: COSMIC on Wayland (`XDG_CURRENT_DESKTOP=COSMIC`, `XDG_SESSION_TYPE=wayland`)
- Rust stable via rustup. `rust-toolchain.toml` asks for rustfmt and clippy.
  The first Cargo command may install those components under `~/.rustup`.
- GTK 4.14 development files were present. The GTK client depends on
  libadwaita 1.5. `libadwaita-1-dev` 1.5.0, `libadwaita-1-0` 1.5.0, and
  `libappstream-dev` are installed. `cargo build -p lda-gtk` succeeds against
  those system headers.
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
`libgtk-4-dev`, `libadwaita-1-dev`, `pkg-config`, and `libsqlite3-dev`.
The IPC test uses a peer-to-peer socket and does not need a session bus.

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

- `zbus` is exactly 5.19. zbus 6 changed the connection builder. The port stays
  deferred until there is a concrete API, maintenance, or security reason.
  Do not combine it with UI work.
- `gtk4` 0.11 with feature `v4_14` only. Do not enable `v4_16` on a 4.14 system.
- `reqwest` uses rustls, not OpenSSL.
- `rusqlite` uses the system SQLite. If that fails to link, switch that crate
  to the bundled feature.

## Indentation

Keep functions shallow. More than three indentation levels is a signal to
return early or extract a function, unless the extra level is actually required.

## Checked while landing 0.2.0

Host: Pop!_OS 24.04 LTS, Linux 7.2.6-76070206-generic, x86_64, COSMIC on Wayland.
The numbers below are from that debug build on that machine. They are not a
budget and they are not a release-build figure.

Startup, until the daemon logged `listening on the session bus`, using a
temporary `--config` and `--data-dir` under `/tmp` (not `~/.config/lda`):

```text
/usr/bin/time -f 'elapsed_sec %e max_rss_kb %M'
elapsed_sec 0.04
```

That elapsed time is the waiter shell, polled every 10 ms. `max_rss_kb 3720`
is that shell. The daemon's own `VmHWM` just after the log line was 21568 kB.
An earlier sample the same way was 0.033 s and 21572 kB. Shutdown still logs
`bus shutdown timed out` and then `stopped`, and the process exits.

`AddNotification` on `org.freedesktop.portal.Desktop` returned success for one
ignored test (`live_notification_when_portal_exists`). The toast itself was
not watched. `lda-gtk` was left running for 4 seconds against that temporary
daemon (`timeout` exit 124), produced no stderr, and the daemon was still
alive afterward. The window was not clicked. Orca was not run. Accessible
names are set on the transcript, the message box, search, and the source list.

`pkg-config --exists gtk4-layer-shell-0` exits 1 on this machine, so
layer-shell stays false. GlobalShortcuts was absent from the portal
introspection when this session was probed. `rpmbuild` and `flatpak-builder`
are not installed, so the RPM spec and the Flatpak manifest were not built.
`dpkg-deb` is `/usr/bin/dpkg-deb`. `packaging/deb/build.sh` wrote
`target/lda_0.2.0_amd64.deb` and that package was not installed. The packaged
unit's `ExecStart` is rewritten to `/usr/bin/lda-daemon`. The copy in
`packaging/systemd/user/` still points at `~/.local/bin` for a manual install.

## Checked while fixing the chat box and voice routing

Host and session are the same COSMIC Wayland machine. Ollama was 0.30.10 and
`ollama list` showed only `mistral:latest`. The checks used a temporary
`--config` and `--data-dir` under `/tmp`. The real `~/.config/lda/config.toml`
mtime did not change. No model was downloaded. Talk was not clicked, and
`pw-record` / `parecord` did not start.

`cargo test --workspace --offline` and
`cargo clippy --workspace --all-targets --offline -- -D warnings` passed
before the last two GTK edits (return focus to the conversation window, and
treat ISO Enter like Return). `cargo build -p lda-gtk --offline` succeeded
after those edits. `cargo test` does not replace `target/debug/lda-gtk` or
`target/debug/lda-cli`; the live check used binaries from an explicit
`cargo build`.

Voice, against that temporary daemon:

- `lda-cli transcribe` with `mistral:latest` failed before any chat request:
  the model does not transcribe speech, and no speech-to-text model is installed.
- `lda-cli speak` with `mistral:latest` failed the same way: it does not
  generate speech, and the text was not sent to chat.
- After reload to `phonon`, `speak` reached the loopback speech endpoint and
  failed with the Ollama 0.40.9 message. This server has no `/v1/audio/speech`.
  The daemon log has no `request started` for that call.
- After reload to `whisper:latest`, `transcribe` posted the wav and Ollama
  answered `model 'whisper:latest' not found`. It did not start a chat turn.
- The model was set back to `mistral:latest`. `lda-cli chat` printed `Pong`.

GTK, on the binary that contains “Talk listens only while it is on.”:

- AT-SPI showed the conversation frame active, the message box focused, Talk
  unchecked, and the connected status that mentions Talk.
- Activating Send with the sentence
  `refine-send-7f3a Reply with the single word pong.` cleared the box, stored
  the user row, and the assistant row was `Pong`.
- A synthetic Return from AT-SPI did not reach the text view, on the default
  GDK backend or with `GDK_BACKEND=x11`. This GTK build does not export
  `gtk_test_widget_send_key`. A physical Enter press was not observed.
  The controller is capture-phase. GTK 4.14 inserts the newline from the text
  view’s own key controller, which stays on the bubble phase. The startup focus
  timer leaves the model dialog alone when that dialog is open. The recorder’s
  stderr is drained on a side thread so a noisy `pw-record` cannot fill the pipe
  and stall. A missing wav is reported as an empty recording.

## Not verified here

- KDE Plasma, XFCE, and X11 sessions. The KDE classification test only checks
  that `tested_here` is false.
- A full click-through of the GTK window, including the Allow dialog, the
  provider editor, the model dialog, source links, and the Talk button.
  The later isolated check activated Send and confirmed the message box had
  focus. It did not press a physical Enter key and it did not open the
  microphone. The earlier 4-second launch, from when libadwaita headers were
  installed, did not click anything.
- Orca or another screen reader.
- A live SearXNG, a live page on the public internet, or a real Anthropic or
  xAI key.
- `open_uri`, `open_directory`, and `open_application` against this desktop.
  The confirmation decision is covered by a scripted provider test.
- Install of the systemd user unit, the D-Bus service file, or the desktop entry.
- An RPM build or a Flatpak build.
