use async_trait::async_trait;
use lda_common::Result;

use crate::SignalBus;

/// Operations the daemon exposes. The D-Bus object delegates to this trait.
#[async_trait]
pub trait AgentBackend: Send + Sync {
    fn signals(&self) -> &SignalBus;

    async fn ping(&self) -> String;

    async fn send_message(
        &self,
        conversation_id: &str,
        provider_id: &str,
        text: &str,
    ) -> Result<String>;

    async fn cancel(&self, request_id: &str) -> Result<()>;

    async fn shutdown(&self) -> Result<()>;

    async fn list_conversations(&self, query: &str) -> Result<String>;

    async fn get_messages(&self, conversation_id: &str) -> Result<String>;

    async fn delete_conversation(&self, conversation_id: &str) -> Result<()>;

    async fn delete_all_conversations(&self) -> Result<()>;

    async fn list_providers(&self) -> Result<String>;

    async fn upsert_provider(&self, provider_json: &str) -> Result<()>;

    async fn set_secret(&self, provider_id: &str, secret: &str) -> Result<()>;

    async fn clear_secret(&self, provider_id: &str) -> Result<()>;

    async fn secret_status(&self, provider_id: &str) -> Result<String>;

    async fn list_models(&self, provider_id: &str) -> Result<String>;

    async fn discover_models(&self, provider_id: &str) -> Result<String>;

    async fn start_model_pull(&self, provider_id: &str, model_name: &str) -> Result<String>;

    async fn model_pull_status(&self) -> Result<String>;

    async fn system_snapshot(&self) -> Result<String>;

    async fn reload_config(&self) -> Result<()>;

    async fn export_data(&self) -> Result<String>;

    async fn confirm_action(&self, request_id: &str, call_id: &str, allow: bool) -> Result<()>;

    async fn list_memories(&self) -> Result<String>;

    async fn remember(&self, text: &str) -> Result<String>;

    async fn forget_memory(&self, id: &str) -> Result<()>;
}
