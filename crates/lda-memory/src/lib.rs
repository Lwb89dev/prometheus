//! Local SQLite store. Secrets are not written here.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lda_common::{
    now_ms, ConversationSummary, Error, MemoryNote, Message, Result, Role, StoredMessage, ToolCall,
    ToolRunRecord,
};
use lda_security::redact_secrets;
use rusqlite::{params, Connection, OptionalExtension};

const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE conversations (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        model TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        tool_calls_json TEXT,
        tool_call_id TEXT,
        tool_name TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX messages_conversation ON messages(conversation_id, created_at);
    CREATE TABLE tool_runs (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL,
        request_id TEXT NOT NULL,
        tool_name TEXT NOT NULL,
        arguments_json TEXT NOT NULL,
        result_json TEXT,
        ok INTEGER NOT NULL,
        error TEXT,
        started_at INTEGER NOT NULL,
        finished_at INTEGER
    );
    CREATE TABLE preferences (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    ",
    "
    CREATE TABLE memories (
        id TEXT PRIMARY KEY,
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    ",
];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            lda_common::private_dir(parent)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(db_err)?;
        let store = Self { conn };
        store.migrate()?;
        let _ = store
            .conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            });
        store
            .conn
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(db_err)?;
        lda_common::tighten_file(path)?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(db_err)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at INTEGER NOT NULL
                );",
            )
            .map_err(db_err)?;
        let current: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let version = (index as i64) + 1;
            if version <= current {
                continue;
            }
            let tx = self.conn.unchecked_transaction().map_err(db_err)?;
            tx.execute_batch(sql).map_err(db_err)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![version, now_ms()],
            )
            .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        Ok(())
    }

    pub fn create_conversation(
        &self,
        id: &str,
        title: &str,
        provider_id: &str,
        model: &str,
    ) -> Result<ConversationSummary> {
        let now = now_ms();
        self.conn
            .execute(
                "INSERT INTO conversations (id, title, provider_id, model, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, title, provider_id, model, now],
            )
            .map_err(db_err)?;
        Ok(ConversationSummary {
            id: id.to_string(),
            title: title.to_string(),
            provider_id: provider_id.to_string(),
            model: model.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn conversation_exists(&self, id: &str) -> Result<bool> {
        let found: Option<String> = self
            .conn
            .query_row("SELECT id FROM conversations WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_err)?;
        Ok(found.is_some())
    }

    pub fn list_conversations(&self, query: &str) -> Result<Vec<ConversationSummary>> {
        let like = like_pattern(query);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT c.id, c.title, c.provider_id, c.model, c.created_at, c.updated_at
                 FROM conversations c
                 LEFT JOIN messages m ON m.conversation_id = c.id
                 WHERE ?1 = ''
                    OR c.title LIKE ?1 ESCAPE '\\'
                    OR m.content LIKE ?1 ESCAPE '\\'
                 ORDER BY c.updated_at DESC
                 LIMIT 200",
            )
            .map_err(db_err)?;
        let rows = stmt.query_map([&like], map_conversation).map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    pub fn recent_messages(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, conversation_id, role, content, tool_calls_json, tool_call_id, tool_name, created_at
                 FROM messages
                 WHERE conversation_id = ?1
                 ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([conversation_id], map_message)
            .map_err(db_err)?;
        let mut messages = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        if messages.len() > limit {
            let skip = messages.len() - limit;
            messages = messages.split_off(skip);
        }
        while messages
            .first()
            .is_some_and(|message| message.role == Role::Tool)
        {
            messages.remove(0);
        }
        Ok(messages)
    }

    pub fn append_message(&self, message: &StoredMessage) -> Result<()> {
        if message.content.len() > lda_common::MAX_STORED_MESSAGE {
            return Err(Error::InvalidInput("message is too large to store".into()));
        }
        let tools = if message.tool_calls.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&message.tool_calls)
                    .map_err(|err| Error::Database(err.to_string()))?,
            )
        };
        self.conn
            .execute(
                "INSERT INTO messages
                    (id, conversation_id, role, content, tool_calls_json, tool_call_id, tool_name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    message.id,
                    message.conversation_id,
                    message.role.as_str(),
                    message.content,
                    tools,
                    message.tool_call_id,
                    message.tool_name,
                    message.created_at,
                ],
            )
            .map_err(db_err)?;
        self.conn
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![message.created_at, message.conversation_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        let count = self
            .conn
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(db_err)?;
        if count == 0 {
            return Err(Error::NotFound(id.to_string()));
        }
        self.conn
            .execute("DELETE FROM tool_runs WHERE conversation_id = ?1", [id])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn delete_all_conversations(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages", [])
            .map_err(db_err)?;
        self.conn
            .execute("DELETE FROM tool_runs", [])
            .map_err(db_err)?;
        self.conn
            .execute("DELETE FROM conversations", [])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn record_tool_run(&self, run: &ToolRunRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO tool_runs
                    (id, conversation_id, request_id, tool_name, arguments_json, result_json, ok, error, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    run.id,
                    run.conversation_id,
                    run.request_id,
                    run.tool_name,
                    run.arguments_json,
                    run.result_json,
                    run.ok as i64,
                    run.error,
                    run.started_at,
                    run.finished_at,
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn export_json(&self) -> Result<serde_json::Value> {
        let conversations = self.list_conversations("")?;
        let mut exported = Vec::new();
        for conversation in conversations {
            let messages = self.recent_messages(&conversation.id, 10_000)?;
            exported.push(serde_json::json!({
                "conversation": conversation,
                "messages": messages,
            }));
        }
        let memories = self.list_memories()?;
        Ok(serde_json::json!({ "conversations": exported, "memories": memories }))
    }

    pub fn purge_older_than(&self, cutoff_ms: i64) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "DELETE FROM conversations WHERE updated_at < ?1",
                [cutoff_ms],
            )
            .map_err(db_err)?;
        Ok(count)
    }

    pub fn path_of(data_dir: &Path) -> PathBuf {
        data_dir.join("lda.db")
    }

    pub fn list_memories(&self) -> Result<Vec<MemoryNote>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, body, created_at FROM memories ORDER BY created_at ASC LIMIT 50")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(MemoryNote {
                    id: row.get(0)?,
                    body: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    pub fn remember(&self, body: &str) -> Result<MemoryNote> {
        let body = clean_memory(body)?;
        if self.list_memories()?.len() >= 50 {
            return Err(Error::InvalidInput(
                "memory is full; delete a note first".into(),
            ));
        }
        let note = MemoryNote {
            id: lda_common::new_id(),
            body,
            created_at: now_ms(),
        };
        self.conn
            .execute(
                "INSERT INTO memories (id, body, created_at) VALUES (?1, ?2, ?3)",
                params![note.id, note.body, note.created_at],
            )
            .map_err(db_err)?;
        Ok(note)
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        if !valid_memory_id(id) {
            return Err(Error::InvalidInput("memory id is not valid".into()));
        }
        let count = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", [id])
            .map_err(db_err)?;
        if count == 0 {
            return Err(Error::NotFound(id.to_string()));
        }
        Ok(())
    }
}

fn clean_memory(body: &str) -> Result<String> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 2_000 {
        return Err(Error::InvalidInput(
            "a memory must be 1 to 2000 characters".into(),
        ));
    }
    if body
        .chars()
        .any(|ch| ch == '\0' || (ch.is_control() && ch != '\n'))
    {
        return Err(Error::InvalidInput(
            "a memory contains unsupported control characters".into(),
        ));
    }
    if redact_secrets(body) != body {
        return Err(Error::InvalidInput(
            "that note looks like a secret and was not stored".into(),
        ));
    }
    Ok(body.to_string())
}

pub fn valid_memory_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 32 && bytes.iter().all(|byte| byte.is_ascii_hexdigit())
}

/// Shared handle so tools and the daemon see the same notes.
pub struct MemoryGate {
    store: std::sync::Arc<Mutex<Store>>,
    enabled: std::sync::atomic::AtomicBool,
}

impl MemoryGate {
    pub fn new(store: std::sync::Arc<Mutex<Store>>, enabled: bool) -> Self {
        Self {
            store,
            enabled: std::sync::atomic::AtomicBool::new(enabled),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn list(&self) -> Result<Vec<MemoryNote>> {
        lock_store(&self.store)?.list_memories()
    }

    pub fn notes_for_prompt(&self) -> Vec<String> {
        if !self.enabled() {
            return Vec::new();
        }
        let Ok(notes) = self.list() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut chars = 0;
        for note in notes {
            if out.len() == 20 || chars + note.body.chars().count() > 4_000 {
                break;
            }
            chars += note.body.chars().count();
            out.push(note.body);
        }
        out
    }

    pub fn remember(&self, body: &str) -> Result<MemoryNote> {
        if !self.enabled() {
            return Err(Error::InvalidInput(
                "persistent memory is disabled in the configuration".into(),
            ));
        }
        lock_store(&self.store)?.remember(body)
    }

    /// Deletes a note even when new notes are disabled, so a saved note can be removed.
    pub fn forget(&self, id: &str) -> Result<()> {
        lock_store(&self.store)?.forget(id)
    }
}

fn lock_store(store: &Mutex<Store>) -> Result<std::sync::MutexGuard<'_, Store>> {
    store
        .lock()
        .map_err(|_| Error::Other("memory store lock is poisoned".into()))
}

fn map_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        provider_id: row.get(2)?,
        model: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    let role_text: String = row.get(2)?;
    let tools_json: Option<String> = row.get(4)?;
    let tool_calls = tools_json
        .as_deref()
        .map(serde_json::from_str::<Vec<ToolCall>>)
        .transpose()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err))
        })?
        .unwrap_or_default();
    Ok(StoredMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: Role::parse(&role_text).unwrap_or(Role::Assistant),
        content: row.get(3)?,
        tool_calls,
        tool_call_id: row.get(5)?,
        tool_name: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn like_pattern(query: &str) -> String {
    if query.trim().is_empty() {
        return String::new();
    }
    let mut escaped = String::new();
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("%{escaped}%")
}

fn db_err(err: rusqlite::Error) -> Error {
    Error::Database(err.to_string())
}

pub fn history_for_model(messages: &[StoredMessage]) -> Vec<Message> {
    messages.iter().map(StoredMessage::as_message).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lda_common::{new_id, title_from};

    #[test]
    fn persists_searches_and_deletes() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        store
            .create_conversation(&id, &title_from("disk full?"), "ollama", "mistral")
            .unwrap();
        store
            .append_message(&StoredMessage {
                id: new_id(),
                conversation_id: id.clone(),
                role: Role::User,
                content: "why is the disk full?".into(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                tool_name: None,
                created_at: now_ms(),
            })
            .unwrap();
        let found = store.list_conversations("disk").unwrap();
        assert_eq!(found.len(), 1);
        assert!(store.list_conversations("battery").unwrap().is_empty());
        let exported = store.export_json().unwrap();
        assert_eq!(exported["conversations"].as_array().unwrap().len(), 1);
        store.delete_conversation(&id).unwrap();
        assert!(store.list_conversations("").unwrap().is_empty());
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lda.db");
        Store::open(&path).unwrap();
        Store::open(&path).unwrap();
    }

    #[test]
    fn memory_is_explicit_and_rejects_secrets() {
        let store = Store::open_in_memory().unwrap();
        let note = store.remember("The printer is on the desk.").unwrap();
        assert_eq!(store.list_memories().unwrap().len(), 1);
        assert!(store.remember("token sk-abcdefghijklmnop").is_err());
        store.forget(&note.id).unwrap();
        assert!(store.list_memories().unwrap().is_empty());
    }

    #[test]
    fn like_escapes_wildcards() {
        assert_eq!(like_pattern("100%_ok"), "%100\\%\\_ok%");
        assert_eq!(like_pattern("  "), "");
    }
}
