use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lda_common::{Message, Result, ToolCall};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::delta::LlmDelta;
use crate::provider::{ChatRequest, LlmProvider, ProviderCapabilities};

pub struct ScriptedRound {
    pub chunks: Vec<String>,
    pub tool_calls: Vec<ToolCall>,
    pub hang: bool,
}

pub struct ScriptedProvider {
    rounds: Mutex<VecDeque<ScriptedRound>>,
    pub seen: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl ScriptedProvider {
    pub fn new(rounds: Vec<ScriptedRound>) -> Self {
        Self {
            rounds: Mutex::new(VecDeque::from(rounds)),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn id(&self) -> &str {
        "scripted"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            temperature: true,
            usage: false,
        }
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["scripted".to_string()])
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<mpsc::Receiver<Result<LlmDelta>>> {
        self.seen.lock().expect("seen").push(request.messages);
        let round = self.rounds.lock().expect("rounds").pop_front();
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let Some(round) = round else {
                let _ = tx
                    .send(Err(lda_common::Error::Provider("no scripted round".into())))
                    .await;
                return;
            };
            if round.hang {
                cancel.cancelled().await;
                let _ = tx.send(Err(lda_common::Error::Cancelled)).await;
                return;
            }
            for chunk in round.chunks {
                if cancel.is_cancelled() {
                    let _ = tx.send(Err(lda_common::Error::Cancelled)).await;
                    return;
                }
                if tx.send(Ok(LlmDelta::Text(chunk))).await.is_err() {
                    return;
                }
            }
            for (index, call) in round.tool_calls.into_iter().enumerate() {
                let arguments = if call.arguments.is_null() {
                    "{}".to_string()
                } else {
                    call.arguments.to_string()
                };
                let delta = LlmDelta::ToolCall {
                    index,
                    id: Some(call.id),
                    name: Some(call.name),
                    arguments: Some(arguments),
                    replace_arguments: true,
                };
                if tx.send(Ok(delta)).await.is_err() {
                    return;
                }
            }
            let _ = tx.send(Ok(LlmDelta::Finished)).await;
        });
        Ok(rx)
    }
}
