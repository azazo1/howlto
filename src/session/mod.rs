use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rig_core::message::Message;
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    agent::{answer::AnswerAgentResponse, submit_commands::CommandItem},
    config::SessionConfig,
    error::Result,
};

const SESSION_FILE_SUFFIX: &str = ".json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub created_at: i64,
    pub updated_at: i64,
    pub cwd: String,
    pub last_prompt: String,
    pub final_text: String,
    pub commands: Vec<CommandItem>,
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new(cwd: &Path, prompt: &str, response: &AnswerAgentResponse) -> Self {
        let now = now_seconds();
        Self {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            cwd: cwd.to_string_lossy().into_owned(),
            last_prompt: prompt.to_string(),
            final_text: response.final_text.clone(),
            commands: response.commands.clone(),
            messages: response.messages.clone(),
        }
    }

    pub fn update(&mut self, prompt: &str, response: &AnswerAgentResponse) {
        if !prompt.trim().is_empty() {
            self.last_prompt = prompt.to_string();
        }
        self.updated_at = now_seconds();
        self.final_text = response.final_text.clone();
        self.commands = response.commands.clone();
        self.messages = response.messages.clone();
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: Uuid,
    pub updated_at: i64,
    pub last_prompt: String,
    pub final_text: String,
    pub command_count: usize,
}

#[derive(Debug, Deserialize)]
struct SessionMeta {
    id: Uuid,
    updated_at: i64,
    #[serde(default)]
    last_prompt: String,
    #[serde(default)]
    final_text: String,
    #[serde(default)]
    commands: Vec<CommandItem>,
}

pub struct SessionStore {
    root: PathBuf,
    config: SessionConfig,
}

impl SessionStore {
    pub fn new(config_dir: impl AsRef<Path>, config: SessionConfig) -> Self {
        Self {
            root: config_dir.as_ref().join("sessions"),
            config,
        }
    }

    pub async fn save(&self, session: &mut Session) -> Result<()> {
        let dir = self.cwd_dir(&session.cwd);
        fs::create_dir_all(&dir).await?;
        set_dir_permissions(&self.root).await;
        set_dir_permissions(&dir).await;
        let path = dir.join(session_file_name(session.id));
        let tmp = dir.join(format!("{}.tmp", session.id));
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&tmp).await?;
        file.write_all(&serde_json::to_vec_pretty(session)?).await?;
        file.flush().await?;
        drop(file);
        fs::rename(&tmp, &path).await?;
        debug!(path = %path.display(), "Session saved.");
        self.cleanup().await;
        Ok(())
    }

    pub async fn load(&self, cwd: &Path, id: Uuid) -> Result<Option<Session>> {
        let path = self.session_path(cwd, id);
        let content = match fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_str(&content) {
            Ok(session) => Ok(Some(session)),
            Err(error) => {
                warn!(path = %path.display(), error = %error, "Ignoring corrupt session file.");
                Ok(None)
            }
        }
    }

    pub async fn list(&self, cwd: &Path) -> Result<Vec<SessionSummary>> {
        let dir = self.cwd_dir(cwd);
        let mut entries = match fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut sessions = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(SESSION_FILE_SUFFIX))
            {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path).await else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<SessionMeta>(&content) else {
                warn!(path = %path.display(), "Ignoring corrupt session summary.");
                continue;
            };
            sessions.push(SessionSummary {
                id: meta.id,
                updated_at: meta.updated_at,
                last_prompt: meta.last_prompt,
                final_text: meta.final_text,
                command_count: meta.commands.len(),
            });
        }
        sessions.sort_by_key(|right| std::cmp::Reverse(right.updated_at));
        Ok(sessions)
    }

    pub async fn cleanup(&self) {
        if !self.root.is_dir() {
            return;
        }
        let mut root_entries = match fs::read_dir(&self.root).await {
            Ok(entries) => entries,
            Err(error) => {
                warn!(path = %self.root.display(), error = %error, "Failed to scan sessions.");
                return;
            }
        };
        let mut all_files = Vec::new();
        while let Ok(Some(entry)) = root_entries.next_entry().await {
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            self.collect_dir_files(&entry.path(), &mut all_files).await;
        }

        let mut report = CleanupReport::default();
        if self.config.ttl_days > 0
            && let Some(cutoff) = self.config.ttl_days.checked_mul(86_400).map(|secs| now_seconds().saturating_sub(secs as i64))
        {
            let (expired, kept): (Vec<_>, Vec<_>) = all_files
                .into_iter()
                .partition(|file| file.updated_at < cutoff);
            all_files = kept;
            for file in expired {
                report.deleted_files += 1;
                report.deleted_bytes += file.size;
                if let Err(error) = fs::remove_file(&file.path).await {
                    warn!(path = %file.path.display(), error = %error, "Failed to remove expired session.");
                }
            }
        }

        if self.config.max_per_dir > 0 {
            all_files.sort_by_key(|file| (file.cwd_dir.clone(), file.updated_at));
            let mut per_dir = Vec::new();
            for file in all_files.drain(..) {
                per_dir.push(file);
            }
            let mut grouped: Vec<(PathBuf, Vec<SessionFile>)> = Vec::new();
            for file in per_dir {
                match grouped.last_mut() {
                    Some((dir, files)) if *dir == file.cwd_dir => files.push(file),
                    _ => grouped.push((file.cwd_dir.clone(), vec![file])),
                }
            }
            for (_, mut files) in grouped {
                if files.len() <= self.config.max_per_dir {
                    all_files.extend(files);
                    continue;
                }
                let remove_count = files.len() - self.config.max_per_dir;
                for file in files.drain(..remove_count) {
                    report.deleted_files += 1;
                    report.deleted_bytes += file.size;
                    if let Err(error) = fs::remove_file(&file.path).await {
                        warn!(path = %file.path.display(), error = %error, "Failed to remove old session.");
                    }
                }
                all_files.extend(files);
            }
        }

        if self.config.max_bytes > 0 {
            let total: u64 = all_files.iter().map(|file| file.size).sum();
            if total > self.config.max_bytes {
                all_files.sort_by_key(|file| file.updated_at);
                let mut total = total;
                for file in all_files.iter().take(all_files.len().saturating_sub(1)) {
                    if total <= self.config.max_bytes {
                        break;
                    }
                    total = total.saturating_sub(file.size);
                    report.deleted_files += 1;
                    report.deleted_bytes += file.size;
                    if let Err(error) = fs::remove_file(&file.path).await {
                        warn!(path = %file.path.display(), error = %error, "Failed to remove old session.");
                    }
                }
            }
        }

        if report.deleted_files > 0 {
            info!(
                deleted_files = report.deleted_files,
                deleted_bytes = report.deleted_bytes,
                "Session cleanup completed."
            );
        }
    }

    async fn collect_dir_files(&self, dir: &Path, out: &mut Vec<SessionFile>) {
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(error) => {
                warn!(path = %dir.display(), error = %error, "Failed to scan session directory.");
                return;
            }
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.ends_with(SESSION_FILE_SUFFIX) || name.ends_with(".tmp") {
                continue;
            }
            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            let updated_at = fs::read_to_string(&path)
                .await
                .ok()
                .and_then(|content| serde_json::from_str::<SessionMeta>(&content).ok())
                .map(|meta| meta.updated_at)
                .unwrap_or_else(|| modified_seconds(&metadata));
            out.push(SessionFile {
                path,
                cwd_dir: dir.to_path_buf(),
                updated_at,
                size: metadata.len(),
            });
        }
    }

    fn cwd_dir(&self, cwd: impl AsRef<Path>) -> PathBuf {
        self.root.join(cwd_key(cwd).to_string())
    }

    fn session_path(&self, cwd: impl AsRef<Path>, id: Uuid) -> PathBuf {
        self.cwd_dir(cwd).join(session_file_name(id))
    }
}

#[derive(Debug, Default)]
struct CleanupReport {
    deleted_files: usize,
    deleted_bytes: u64,
}

#[derive(Debug)]
struct SessionFile {
    path: PathBuf,
    cwd_dir: PathBuf,
    updated_at: i64,
    size: u64,
}

fn session_file_name(id: Uuid) -> String {
    format!("{id}{SESSION_FILE_SUFFIX}")
}

fn cwd_key(cwd: impl AsRef<Path>) -> Uuid {
    let cwd = cwd.as_ref();
    let absolute = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        absolute.to_string_lossy().as_bytes(),
    )
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn modified_seconds(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

async fn set_dir_permissions(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await {
            warn!(path = %dir.display(), error = %error, "Failed to harden session directory permissions.");
        }
    }
}

#[cfg(test)]
mod tests {
    use rig_core::{OneOrMany, message::{Message, UserContent, Text}};
    use tokio::fs;
    use uuid::Uuid;

    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("howlto-session-test-{}", Uuid::new_v4()))
    }

    fn sample_session(cwd: &Path, prompt: &str, updated_at: i64) -> Session {
        Session {
            id: Uuid::new_v4(),
            created_at: updated_at,
            updated_at,
            cwd: cwd.to_string_lossy().into_owned(),
            last_prompt: prompt.to_string(),
            final_text: format!("answer for {prompt}"),
            commands: vec![CommandItem {
                command: "printf ok".into(),
                description: "test".into(),
            }],
            messages: vec![Message::User {
                content: OneOrMany::one(UserContent::Text(Text::new(prompt))),
            }],
        }
    }

    #[tokio::test]
    async fn save_load_and_list_round_trip() {
        let root = temp_dir();
        let store = SessionStore::new(&root, SessionConfig::default());
        let cwd = std::env::current_dir().unwrap();
        let mut session = sample_session(&cwd, "first", now_seconds());

        store.save(&mut session).await.unwrap();
        let loaded = store.load(&cwd, session.id).await.unwrap().unwrap();
        assert_eq!(loaded.last_prompt, "first");
        assert_eq!(loaded.commands[0].command, "printf ok");
        assert_eq!(loaded.messages.len(), 1);

        let summaries = store.list(&cwd).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, session.id);
        fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn list_sorts_newest_first() {
        let root = temp_dir();
        let store = SessionStore::new(&root, SessionConfig::default());
        let cwd = std::env::current_dir().unwrap();
        let mut old = sample_session(&cwd, "old", now_seconds() - 100);
        let mut new = sample_session(&cwd, "new", now_seconds());
        store.save(&mut old).await.unwrap();
        store.save(&mut new).await.unwrap();

        let summaries = store.list(&cwd).await.unwrap();
        assert_eq!(summaries[0].id, new.id);
        assert_eq!(summaries[1].id, old.id);
        fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn corrupt_session_is_skipped() {
        let root = temp_dir();
        let store = SessionStore::new(&root, SessionConfig::default());
        let cwd = std::env::current_dir().unwrap();
        let dir = store.cwd_dir(&cwd);
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join(format!("{}.json", Uuid::new_v4())), "not json").await.unwrap();

        assert!(store.list(&cwd).await.unwrap().is_empty());
        fs::remove_dir_all(&root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_files_and_directories_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir();
        let store = SessionStore::new(&root, SessionConfig::default());
        let cwd = std::env::current_dir().unwrap();
        let mut session = sample_session(&cwd, "private", now_seconds());
        store.save(&mut session).await.unwrap();

        let file_permissions = fs::metadata(store.session_path(&cwd, session.id))
            .await
            .unwrap()
            .permissions();
        assert_eq!(file_permissions.mode() & 0o777, 0o600);
        let dir_permissions = fs::metadata(store.cwd_dir(&cwd)).await.unwrap().permissions();
        assert_eq!(dir_permissions.mode() & 0o777, 0o700);
        fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_enforces_ttl_per_dir_and_total_size() {
        let root = temp_dir();
        let config = SessionConfig {
            max_bytes: 300,
            max_per_dir: 2,
            ttl_days: 1,
        };
        let store = SessionStore::new(&root, config);
        let cwd = std::env::current_dir().unwrap();
        let mut expired = sample_session(&cwd, "expired", now_seconds() - 86_400 - 1);
        let mut oldest = sample_session(&cwd, "oldest", now_seconds() - 10);
        let mut newest = sample_session(&cwd, "newest", now_seconds());
        store.save(&mut expired).await.unwrap();
        store.save(&mut oldest).await.unwrap();
        store.save(&mut newest).await.unwrap();

        let summaries = store.list(&cwd).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, newest.id);
        fs::remove_dir_all(&root).await.unwrap();
    }
}
