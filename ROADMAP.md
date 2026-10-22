# Roadmap

Version 0.2.0 contains Milestones 0 through 5 as working code, with the limits
named in each section. A feature that was not run on a desktop is not described
as supported there.

## Milestone 0 and 1

Workspace, security boundaries, unprivileged daemon, GTK conversation window,
Ollama and OpenAI-compatible streaming, SQLite, read-only system tools, and
session-bus IPC. Tagged `v0.1.0`.

## Milestone 2 — Agent capabilities

In the tree: SearXNG or a JSON `results` endpoint you configure, HTML
extraction with timeouts, size caps, and a rate limit, untrusted page text,
and a source list in the window. Tool runs stay in the audit table. Search is
disabled until `[web]` enables it, and a public SearXNG is not the default.

## Milestone 3 — Desktop integration

In the tree: portal capability flags, `AddNotification`, and `OpenURI`.
The notification call returned success once on this COSMIC Wayland session.
GlobalShortcuts is reported when the interface exists and is not registered
here. Layer-shell is reported only when `gtk4-layer-shell-0` is installed.
Otherwise the mark stays in a normal GTK window. KDE and XFCE were not run.

## Milestone 4 — More hosts and actions

In the tree: Anthropic and xAI on `LlmProvider`, confirmed user actions,
explicit memories, and `NoVoice`. There is no polkit helper. Packaging recipes
are `packaging/deb/build.sh`, `packaging/rpm/lda.spec`, and
`packaging/flatpak/org.linuxdesktopagent.Gtk.yml`. The Flatpak manifest does
not grant host filesystem access. See `DEVELOPMENT.md` for which of those
recipes were executed.

## Milestone 5 — Stabilization

Permission, path, web, and confirmation tests stay in `cargo test`. The GTK
transcript, message box, search field, and source list have accessible names.
Orca was not run. The startup measurement and the desktops that were not run
are in `DEVELOPMENT.md`.

## Decisions still open

- Install `libadwaita-1-dev` and move the window to libadwaita, or stay on GTK 4.
- Port from zbus 5.19 to zbus 6, or stay on the pin.
- Whether the default local model name should stay `mistral:latest`.
