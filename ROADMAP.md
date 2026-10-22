# Roadmap

Milestone 0 and Milestone 1 are the current tree: workspace, security
boundaries, daemon, GTK conversation window, Ollama, OpenAI-compatible
streaming, SQLite, read-only system tools, and session-bus IPC.

Later milestones stay out of the tree until they are real.

## Milestone 2 — Agent capabilities

- Web search behind a configured SearXNG you run yourself, or another API you
  configure. Do not call public SearXNG instances.
- Fetch and extract page text with timeouts, size caps, and rate limits.
- Treat page text as untrusted. Never execute instructions found in it.
- Show source links separately from model prose.
- Keep tool runs in the existing audit table.

## Milestone 3 — Desktop integration

- Global shortcut where a portal or a tested desktop mechanism exists.
  Suggested default Super+Space only when it is free.
- Floating widget only where layer-shell or a documented equivalent exists.
  Otherwise keep the current window.
- COSMIC and GNOME adapters, tested on those sessions.
- Notifications through the notification portal.
- Document Wayland limits in the adapter, not as a blanket claim.

## Milestone 4 — More hosts and actions

- KDE Plasma and XFCE adapters, after they are tested.
- Anthropic and xAI adapters on the same `LlmProvider` trait.
- User-confirmed actions above read-only, with the action shown before it runs.
- Polkit only through a narrow helper. The daemon stays unprivileged.
- Optional persistent memories that the user can see and delete. Do not infer
  them silently.
- Voice trait, local, push-to-talk, off by default.
- Packaging: `.deb` and RPM. Flatpak only if the sandbox still explains which
  diagnostics it cannot do. Do not weaken the sandbox quietly.

## Milestone 5 — Stabilization

- Permission and path tests kept in CI.
- Accessibility pass on the GTK window.
- Cross-desktop notes in `DEVELOPMENT.md` that say what was actually run.
- No invented performance numbers. Measure before writing any.

## Decisions still open

- Install `libadwaita-1-dev` and move the window to libadwaita, or stay on GTK 4.
- Port from zbus 5.19 to zbus 6, or stay on the pin.
- Whether the default local model name should stay `mistral:latest`.
