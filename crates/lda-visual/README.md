# Prometeus visual entity

`lda-visual` is the native GTK4/Cairo visual layer. It reconstructs the supplied
concept procedurally as independent ring, circuit, terminal, chip, core, flame,
highlight, illumination, and ember layers. The original reference bitmap was
not present in the workspace, so the geometry is an approximation rather than
a pixel-exact trace.

The state model keeps assistant activity, connection, pointer/focus, visibility,
permission waiting, concurrent tools, and temporary feedback orthogonal. The
scene is sampled from monotonic time, so cancellation and rapid transitions do
not leave an animation timeline running in an intermediate state.

## GTK integration

Create one `EntityWidget`, add `widget()` to the window, and forward only events
that are known to have happened:

```rust,no_run
use lda_visual::{DaemonVisualEvent, EntityEvent, EntityWidget};

let entity = EntityWidget::new(192);
entity.dispatch(EntityEvent::Initialized);

// After SendMessage returns successfully:
entity.apply_daemon_event(DaemonVisualEvent::RequestAccepted);
// Forward StreamDelta, parsed StreamEvent, and StreamDone in the same way.
```

The host owns conversation-window activation, its native context menu, and
window placement; these are exposed as `EntityCallbacks`. This keeps the visual
crate from inventing application actions. On Wayland, arbitrary global window
coordinates are intentionally not promised. The persisted `Placement` uses a
corner anchor and normalized offsets so the host can implement a compositor-
appropriate fallback and clamp it to the current monitor work area.

Voice mode is off by default. `MicrophoneLevel` accepts a normalized amplitude
from a separately authorized capture pipeline; it includes a noise gate and
attack/release smoothing. The visual crate never opens a microphone itself.

## Development preview

The preview is explicitly simulated and does not emit backend events:

```sh
cargo run -p lda-visual --example preview
```

It exposes all currently representable states plus simulated microphone level.
Listening and transcription remain preview/manual inputs until the application
has a real, user-authorized audio source.
