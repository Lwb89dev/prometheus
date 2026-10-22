//! One-shot confirmation. The agent publishes the question, then waits here.
//! The daemon's event loop must return to `recv` before this future resolves,
//! so a D-Bus `confirm_action` can answer without sharing the agent task.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Cancelled,
}

pub struct ConfirmHub {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl ConfirmHub {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Returns false when nothing is waiting for this key.
    pub fn decide(&self, key: &str, allow: bool) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(sender) = pending.remove(key) else {
            return false;
        };
        sender.send(allow).is_ok()
    }

    pub async fn ask(&self, key: String, cancel: &CancellationToken, wait: Duration) -> Answer {
        let (sender, receiver) = oneshot::channel();
        if self.pending.lock().is_ok_and(|mut pending| {
            pending.insert(key.clone(), sender);
            true
        }) {
            return self.wait_for(key, receiver, cancel, wait).await;
        }
        Answer::No
    }

    async fn wait_for(
        &self,
        key: String,
        receiver: oneshot::Receiver<bool>,
        cancel: &CancellationToken,
        wait: Duration,
    ) -> Answer {
        let answer = tokio::select! {
            _ = cancel.cancelled() => Answer::Cancelled,
            result = tokio::time::timeout(wait, receiver) => match result {
                Ok(Ok(true)) => Answer::Yes,
                Ok(Ok(false)) | Ok(Err(_)) | Err(_) => Answer::No,
            },
        };
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&key);
        }
        answer
    }
}

impl Default for ConfirmHub {
    fn default() -> Self {
        Self::new()
    }
}
