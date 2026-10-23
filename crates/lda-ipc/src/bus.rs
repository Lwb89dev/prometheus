use std::sync::Arc;

use lda_common::{Error, BUS_NAME, BUS_PATH};
use zbus::fdo;
use zbus::interface;
use zbus::proxy;
use zbus::Connection;

use crate::backend::AgentBackend;

pub struct AgentObject {
    backend: Arc<dyn AgentBackend>,
}

impl AgentObject {
    pub fn new(backend: Arc<dyn AgentBackend>) -> Self {
        Self { backend }
    }

    fn attach(&self, conn: &Connection) {
        self.backend.signals().attach(conn.clone());
    }
}

#[interface(name = "org.linuxdesktopagent.Agent1")]
impl AgentObject {
    async fn ping(&self, #[zbus(connection)] conn: &Connection) -> String {
        self.attach(conn);
        self.backend.ping().await
    }

    async fn send_message(
        &self,
        #[zbus(connection)] conn: &Connection,
        conversation_id: &str,
        provider_id: &str,
        text: &str,
    ) -> fdo::Result<String> {
        self.attach(conn);
        self.backend
            .send_message(conversation_id, provider_id, text)
            .await
            .map_err(dbus_err)
    }

    async fn cancel(&self, request_id: &str) -> fdo::Result<()> {
        self.backend.cancel(request_id).await.map_err(dbus_err)
    }

    async fn shutdown(&self) -> fdo::Result<()> {
        self.backend.shutdown().await.map_err(dbus_err)
    }

    async fn list_conversations(&self, query: &str) -> fdo::Result<String> {
        self.backend
            .list_conversations(query)
            .await
            .map_err(dbus_err)
    }

    async fn get_messages(&self, conversation_id: &str) -> fdo::Result<String> {
        self.backend
            .get_messages(conversation_id)
            .await
            .map_err(dbus_err)
    }

    async fn delete_conversation(&self, conversation_id: &str) -> fdo::Result<()> {
        self.backend
            .delete_conversation(conversation_id)
            .await
            .map_err(dbus_err)
    }

    async fn delete_all_conversations(&self) -> fdo::Result<()> {
        self.backend
            .delete_all_conversations()
            .await
            .map_err(dbus_err)
    }

    async fn list_providers(&self) -> fdo::Result<String> {
        self.backend.list_providers().await.map_err(dbus_err)
    }

    async fn upsert_provider(&self, provider_json: &str) -> fdo::Result<()> {
        self.backend
            .upsert_provider(provider_json)
            .await
            .map_err(dbus_err)
    }

    async fn set_secret(&self, provider_id: &str, secret: &str) -> fdo::Result<()> {
        self.backend
            .set_secret(provider_id, secret)
            .await
            .map_err(dbus_err)
    }

    async fn clear_secret(&self, provider_id: &str) -> fdo::Result<()> {
        self.backend
            .clear_secret(provider_id)
            .await
            .map_err(dbus_err)
    }

    async fn secret_status(&self, provider_id: &str) -> fdo::Result<String> {
        self.backend
            .secret_status(provider_id)
            .await
            .map_err(dbus_err)
    }

    async fn list_models(&self, provider_id: &str) -> fdo::Result<String> {
        self.backend
            .list_models(provider_id)
            .await
            .map_err(dbus_err)
    }

    async fn discover_models(&self, provider_id: &str) -> fdo::Result<String> {
        self.backend
            .discover_models(provider_id)
            .await
            .map_err(dbus_err)
    }

    async fn start_model_pull(&self, provider_id: &str, model_name: &str) -> fdo::Result<String> {
        self.backend
            .start_model_pull(provider_id, model_name)
            .await
            .map_err(dbus_err)
    }

    async fn model_pull_status(&self) -> fdo::Result<String> {
        self.backend.model_pull_status().await.map_err(dbus_err)
    }

    async fn system_snapshot(&self) -> fdo::Result<String> {
        self.backend.system_snapshot().await.map_err(dbus_err)
    }

    async fn reload_config(&self) -> fdo::Result<()> {
        self.backend.reload_config().await.map_err(dbus_err)
    }

    async fn export_data(&self) -> fdo::Result<String> {
        self.backend.export_data().await.map_err(dbus_err)
    }

    async fn confirm_action(
        &self,
        request_id: &str,
        call_id: &str,
        allow: bool,
    ) -> fdo::Result<()> {
        self.backend
            .confirm_action(request_id, call_id, allow)
            .await
            .map_err(dbus_err)
    }

    async fn list_memories(&self) -> fdo::Result<String> {
        self.backend.list_memories().await.map_err(dbus_err)
    }

    async fn remember(&self, text: &str) -> fdo::Result<String> {
        self.backend.remember(text).await.map_err(dbus_err)
    }

    async fn forget_memory(&self, id: &str) -> fdo::Result<()> {
        self.backend.forget_memory(id).await.map_err(dbus_err)
    }
}

fn dbus_err(err: Error) -> fdo::Error {
    fdo::Error::Failed(err.to_string())
}

/// Own the well-known session name and serve the agent object.
///
/// Fails with [`zbus::Error::NameTaken`] when another daemon already owns the name.
/// The builder always requests the name without queueing.
pub async fn serve_session(backend: Arc<dyn AgentBackend>) -> zbus::Result<Connection> {
    zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(BUS_PATH, AgentObject::new(backend))?
        .build()
        .await
}

/// Connect to the session bus and the running daemon.
pub async fn session_proxy() -> zbus::Result<AgentProxy<'static>> {
    let conn = zbus::connection::Builder::session()?
        .max_queued(512)
        .build()
        .await?;
    AgentProxy::new(&conn).await
}

#[proxy(
    gen_blocking = false,
    interface = "org.linuxdesktopagent.Agent1",
    default_service = "org.linuxdesktopagent.Daemon",
    default_path = "/org/linuxdesktopagent/Agent"
)]
pub trait Agent {
    fn ping(&self) -> zbus::Result<String>;

    fn send_message(
        &self,
        conversation_id: &str,
        provider_id: &str,
        text: &str,
    ) -> zbus::Result<String>;

    fn cancel(&self, request_id: &str) -> zbus::Result<()>;

    fn shutdown(&self) -> zbus::Result<()>;

    fn list_conversations(&self, query: &str) -> zbus::Result<String>;

    fn get_messages(&self, conversation_id: &str) -> zbus::Result<String>;

    fn delete_conversation(&self, conversation_id: &str) -> zbus::Result<()>;

    fn delete_all_conversations(&self) -> zbus::Result<()>;

    fn list_providers(&self) -> zbus::Result<String>;

    fn upsert_provider(&self, provider_json: &str) -> zbus::Result<()>;

    fn set_secret(&self, provider_id: &str, secret: &str) -> zbus::Result<()>;

    fn clear_secret(&self, provider_id: &str) -> zbus::Result<()>;

    fn secret_status(&self, provider_id: &str) -> zbus::Result<String>;

    fn list_models(&self, provider_id: &str) -> zbus::Result<String>;

    fn discover_models(&self, provider_id: &str) -> zbus::Result<String>;

    fn start_model_pull(&self, provider_id: &str, model_name: &str) -> zbus::Result<String>;

    fn model_pull_status(&self) -> zbus::Result<String>;

    fn system_snapshot(&self) -> zbus::Result<String>;

    fn reload_config(&self) -> zbus::Result<()>;

    fn export_data(&self) -> zbus::Result<String>;

    fn confirm_action(&self, request_id: &str, call_id: &str, allow: bool) -> zbus::Result<()>;

    fn list_memories(&self) -> zbus::Result<String>;

    fn remember(&self, text: &str) -> zbus::Result<String>;

    fn forget_memory(&self, id: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn stream_delta(&self, request_id: &str, text: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn stream_event(&self, request_id: &str, event_json: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn stream_done(&self, request_id: &str, status: &str, error: &str) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use futures_util::StreamExt;
    use lda_common::{Result, BUS_NAME, BUS_PATH};
    use tokio::net::UnixStream;

    use super::*;
    use crate::SignalBus;

    struct Fake {
        signals: SignalBus,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AgentBackend for Fake {
        fn signals(&self) -> &SignalBus {
            &self.signals
        }

        async fn ping(&self) -> String {
            "ok".to_string()
        }

        async fn send_message(
            &self,
            conversation_id: &str,
            _provider_id: &str,
            text: &str,
        ) -> Result<String> {
            self.seen.lock().expect("seen").push(text.to_string());
            let signals = self.signals.clone();
            tokio::spawn(async move {
                signals.delta("req-1", "hello").await;
                signals.done("req-1", "ok", "").await;
            });
            Ok(format!(
                r#"{{"request_id":"req-1","conversation_id":"{conversation_id}"}}"#
            ))
        }

        async fn cancel(&self, _request_id: &str) -> Result<()> {
            Ok(())
        }

        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }

        async fn list_conversations(&self, _query: &str) -> Result<String> {
            Ok("[]".to_string())
        }

        async fn get_messages(&self, _conversation_id: &str) -> Result<String> {
            Ok("[]".to_string())
        }

        async fn delete_conversation(&self, _conversation_id: &str) -> Result<()> {
            Err(Error::NotFound("missing".into()))
        }

        async fn delete_all_conversations(&self) -> Result<()> {
            Ok(())
        }

        async fn list_providers(&self) -> Result<String> {
            Ok("[]".to_string())
        }

        async fn upsert_provider(&self, _provider_json: &str) -> Result<()> {
            Ok(())
        }

        async fn set_secret(&self, _provider_id: &str, _secret: &str) -> Result<()> {
            Ok(())
        }

        async fn clear_secret(&self, _provider_id: &str) -> Result<()> {
            Ok(())
        }

        async fn secret_status(&self, _provider_id: &str) -> Result<String> {
            Ok("absent".to_string())
        }

        async fn list_models(&self, _provider_id: &str) -> Result<String> {
            Ok("[]".to_string())
        }

        async fn discover_models(&self, _provider_id: &str) -> Result<String> {
            Ok(r#"{"installed":[],"suggestions":[],"ram_mib":0,"notice":""}"#.to_string())
        }

        async fn start_model_pull(&self, _provider_id: &str, _model_name: &str) -> Result<String> {
            Ok(r#"{"phase":"idle","name":"","detail":""}"#.to_string())
        }

        async fn model_pull_status(&self) -> Result<String> {
            Ok(r#"{"phase":"idle","name":"","detail":""}"#.to_string())
        }

        async fn system_snapshot(&self) -> Result<String> {
            Ok("{}".to_string())
        }

        async fn reload_config(&self) -> Result<()> {
            Ok(())
        }

        async fn export_data(&self) -> Result<String> {
            Ok(r#"{"conversations":[]}"#.to_string())
        }

        async fn confirm_action(
            &self,
            _request_id: &str,
            _call_id: &str,
            _allow: bool,
        ) -> Result<()> {
            Ok(())
        }

        async fn list_memories(&self) -> Result<String> {
            Ok("[]".to_string())
        }

        async fn remember(&self, _text: &str) -> Result<String> {
            Ok("{}".to_string())
        }

        async fn forget_memory(&self, _id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn p2p_ping_streams_and_maps_errors() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), round_trip()).await;
        result.expect("ipc test timed out");
    }

    async fn round_trip() {
        let (server_stream, client_stream) = UnixStream::pair().unwrap();
        let guid = zbus::Guid::generate();
        let backend = Arc::new(Fake {
            signals: SignalBus::default(),
            seen: Mutex::new(Vec::new()),
        });
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .p2p()
            .server(guid)
            .unwrap()
            .name(BUS_NAME)
            .unwrap()
            .serve_at(BUS_PATH, AgentObject::new(backend))
            .unwrap()
            .build();
        let client = zbus::connection::Builder::unix_stream(client_stream)
            .p2p()
            .build();
        let (server, client) = tokio::try_join!(server, client).unwrap();
        let _ = server;
        let proxy = AgentProxy::builder(&client)
            .destination(BUS_NAME)
            .unwrap()
            .path(BUS_PATH)
            .unwrap()
            .build()
            .await
            .unwrap();

        assert_eq!(proxy.ping().await.unwrap(), "ok");
        let mut deltas = proxy.receive_stream_delta().await.unwrap();
        let mut dones = proxy.receive_stream_done().await.unwrap();
        let reply = proxy.send_message("abc", "ollama", "hi").await.unwrap();
        assert!(reply.contains("req-1"));

        let delta = tokio::time::timeout(std::time::Duration::from_secs(2), deltas.next())
            .await
            .expect("delta timed out")
            .expect("delta stream ended");
        let args = delta.args().unwrap();
        assert_eq!(args.request_id, "req-1");
        assert_eq!(args.text, "hello");

        let done = tokio::time::timeout(std::time::Duration::from_secs(2), dones.next())
            .await
            .expect("done timed out")
            .expect("done stream ended");
        let args = done.args().unwrap();
        assert_eq!(args.status, "ok");

        let err = proxy.delete_conversation("missing").await.unwrap_err();
        assert!(err.to_string().contains("missing") || err.to_string().contains("not found"));
    }
}
