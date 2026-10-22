//! Desktop capabilities that were actually probed.
//!
//! Global shortcuts and layer-shell are reported only when the portal or the
//! library is present. This crate does not register Super+Space by itself.
//! Voice capture is not linked. The microphone stays closed.

#![forbid(unsafe_code)]

mod portal;
mod voice;

pub use portal::{notify, open_uri, probe, session_note, HostCaps};
pub use voice::{NoVoice, VoiceBackend};

use lda_system::{classify_desktop, DesktopProbe};

/// What this process can see right now. Missing pieces stay false.
pub async fn current_caps() -> HostCaps {
    let info = classify_desktop(&DesktopProbe::from_env());
    probe(&info).await
}
