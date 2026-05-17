use std::path::{Path, PathBuf};

use rig::message::Message;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::info;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub role: TranscriptRole,
    pub markdown: String,
    #[serde(default)]
    pub commands: Vec<crate::agent::tools::CommandCandidate>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub transcript: Vec<TranscriptEntry>,
}

impl ChatSession {
    pub fn new() -> Self {
        let now = chrono_like_now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now.clone(),
            updated_at: now,
            messages: Vec::new(),
            transcript: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = chrono_like_now();
    }
}

impl Default for ChatSession {
    fn default() -> Self {
        Self::new()
    }
}

fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub async fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root).await?;
        Ok(())
    }

    pub fn session_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    pub async fn save(&self, session: &ChatSession) -> Result<()> {
        self.ensure_dir().await?;
        let content = serde_json::to_vec_pretty(session)?;
        fs::write(self.session_path(&session.id), content).await?;
        info!(session_id = %session.id, "saved chat session");
        Ok(())
    }

    pub async fn load(&self, id: &str) -> Result<ChatSession> {
        let content = fs::read(self.session_path(id)).await?;
        Ok(serde_json::from_slice(&content)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::fs;

    use super::{ChatSession, SessionStore, TranscriptEntry, TranscriptRole};
    use crate::agent::tools::CommandCandidate;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("howlto-session-test-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn save_and_load_session_roundtrip() {
        let root = test_root();
        let store = SessionStore::new(&root);
        let mut session = ChatSession::new();
        session.transcript.push(TranscriptEntry {
            role: TranscriptRole::Assistant,
            markdown: "reply".into(),
            commands: vec![CommandCandidate {
                command: "ls -la".into(),
                summary: "list files".into(),
            }],
        });
        store.save(&session).await.unwrap();

        let loaded = store.load(&session.id).await.unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.transcript.len(), 1);
        assert_eq!(loaded.transcript[0].role, TranscriptRole::Assistant);
        assert_eq!(loaded.transcript[0].commands.len(), 1);

        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn load_missing_session_fails() {
        let root = test_root();
        let store = SessionStore::new(&root);
        assert!(store.load("missing").await.is_err());
    }
}
