//! Session-bus API between the daemon and its clients.
//!
//! Payloads that are not plain strings are JSON. The daemon is the only writer
//! of configuration and conversation state. Clients subscribe to signals before
//! they call [`AgentProxy::send_message`], then reload messages when
//! `StreamDone` arrives so a missed delta can be recovered from the database.

#![forbid(unsafe_code)]

mod backend;
mod bus;
mod signals;

pub use backend::AgentBackend;
pub use bus::{
    serve_session, session_proxy, AgentObject, AgentProxy, StreamDelta, StreamDeltaStream,
    StreamDone, StreamDoneStream, StreamEvent as BusStreamEvent, StreamEventStream,
};
pub use signals::SignalBus;
