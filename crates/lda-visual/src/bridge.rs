use lda_common::StreamEvent;

use crate::{Connection, EntityEvent, EntityModel};

/// Inputs that the GTK D-Bus client can forward without inventing backend state.
#[derive(Debug, Clone)]
pub enum DaemonVisualEvent<'a> {
    Connected,
    Disconnected,
    RequestAccepted,
    Delta,
    Event(&'a StreamEvent),
    Done { status: &'a str, error: &'a str },
}

impl EntityModel {
    pub fn apply_daemon_event(&mut self, event: DaemonVisualEvent<'_>, now: f64) {
        match event {
            DaemonVisualEvent::Connected => {
                self.dispatch(EntityEvent::SetConnection(Connection::Online), now)
            }
            DaemonVisualEvent::Disconnected => {
                self.dispatch(EntityEvent::SetConnection(Connection::Offline), now)
            }
            DaemonVisualEvent::RequestAccepted => self.dispatch(EntityEvent::RequestStarted, now),
            DaemonVisualEvent::Delta => self.dispatch(EntityEvent::TextDelta, now),
            DaemonVisualEvent::Event(StreamEvent::ToolStarted { call_id, name }) => {
                self.dispatch(EntityEvent::ToolStarted { id: call_id, name }, now)
            }
            DaemonVisualEvent::Event(StreamEvent::ToolFinished { call_id, ok, .. }) => self
                .dispatch(
                    EntityEvent::ToolFinished {
                        id: call_id,
                        ok: *ok,
                    },
                    now,
                ),
            DaemonVisualEvent::Event(StreamEvent::ReplaceText { .. }) => {}
            DaemonVisualEvent::Done { status, error } => match status {
                "ok" => self.dispatch(EntityEvent::RequestSucceeded, now),
                "cancelled" => self.dispatch(EntityEvent::RequestCancelled, now),
                _ => self.dispatch(
                    EntityEvent::RequestFailed {
                        recoverable: !error.to_ascii_lowercase().contains("permission"),
                    },
                    now,
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Activity, FeedbackKind};

    #[test]
    fn maps_only_real_stream_events() {
        let mut model = EntityModel::new(0.0);
        model.apply_daemon_event(DaemonVisualEvent::RequestAccepted, 1.0);
        assert_eq!(model.sample(1.1).activity, Activity::Thinking);
        model.apply_daemon_event(DaemonVisualEvent::Delta, 2.0);
        assert_eq!(model.sample(2.1).activity, Activity::Responding);
        model.apply_daemon_event(
            DaemonVisualEvent::Done {
                status: "cancelled",
                error: "",
            },
            3.0,
        );
        assert_eq!(model.sample(3.1).feedback, None);
        model.apply_daemon_event(
            DaemonVisualEvent::Done {
                status: "ok",
                error: "",
            },
            4.0,
        );
        assert_eq!(model.sample(4.1).feedback, Some(FeedbackKind::Success));
    }
}
