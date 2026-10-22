use std::sync::{Arc, OnceLock};

use lda_common::{StreamEvent, BUS_INTERFACE, BUS_PATH};
use zbus::Connection;

/// Emits the three stream signals. Safe to clone; the connection is attached once.
#[derive(Clone, Default)]
pub struct SignalBus {
    conn: Arc<OnceLock<Connection>>,
}

impl SignalBus {
    pub fn attach(&self, conn: Connection) {
        let _ = self.conn.set(conn);
    }

    pub async fn delta(&self, request_id: &str, text: &str) {
        let body = (request_id.to_string(), text.to_string());
        self.emit_pair("StreamDelta", &body).await;
    }

    pub async fn event(&self, request_id: &str, event: &StreamEvent) {
        let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
        let body = (request_id.to_string(), json);
        self.emit_pair("StreamEvent", &body).await;
    }

    pub async fn done(&self, request_id: &str, status: &str, error: &str) {
        let body = (
            request_id.to_string(),
            status.to_string(),
            error.to_string(),
        );
        self.emit_triple("StreamDone", &body).await;
    }

    async fn emit_pair(&self, member: &str, body: &(String, String)) {
        let Some(conn) = self.conn.get() else {
            tracing::warn!(member, "dropped a signal because the bus is not attached");
            return;
        };
        if let Err(err) = conn
            .emit_signal(Option::<&str>::None, BUS_PATH, BUS_INTERFACE, member, body)
            .await
        {
            tracing::warn!(member, %err, "failed to emit a signal");
        }
    }

    async fn emit_triple(&self, member: &str, body: &(String, String, String)) {
        let Some(conn) = self.conn.get() else {
            tracing::warn!(member, "dropped a signal because the bus is not attached");
            return;
        };
        if let Err(err) = conn
            .emit_signal(Option::<&str>::None, BUS_PATH, BUS_INTERFACE, member, body)
            .await
        {
            tracing::warn!(member, %err, "failed to emit a signal");
        }
    }
}
