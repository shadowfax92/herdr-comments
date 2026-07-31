use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::format::{normalize_note, normalize_source};
use crate::model::{Comment, Draft, ReviewSession, SCHEMA_VERSION};

const TRANSIENT_TTL_MS: u64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        ensure_private_dir(&root)?;
        for name in ["drafts", "collections", "reviews"] {
            ensure_private_dir(&root.join(name))?;
        }
        Ok(Self { root })
    }

    pub fn create_draft(&self, source_text: &str, pane_id: &str, scope: &str) -> Result<Draft> {
        self.create_draft_at(source_text, pane_id, scope, now_ms())
    }

    fn create_draft_at(
        &self,
        source_text: &str,
        pane_id: &str,
        scope: &str,
        created_at_ms: u64,
    ) -> Result<Draft> {
        let source_text = normalize_source(source_text)?;
        if pane_id.trim().is_empty() || !valid_scope(scope) {
            bail!("draft context is invalid");
        }
        let draft = Draft {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            source_text,
            pane_id: pane_id.to_owned(),
            scope: scope.to_owned(),
            created_at_ms,
        };
        atomic_json(&self.draft_path(&draft.id)?, &draft)?;
        Ok(draft)
    }

    pub fn load_draft(&self, id: &str) -> Result<Draft> {
        let draft: Draft = read_json(&self.draft_path(id)?)?;
        validate_model(&draft.id, id, draft.schema_version)?;
        Ok(draft)
    }

    pub fn delete_draft(&self, id: &str) -> Result<()> {
        remove_private_file(&self.draft_path(id)?)
    }

    pub fn add_comment(&self, scope: &str, source_text: &str, note: &str) -> Result<Comment> {
        if !valid_scope(scope) {
            bail!("comment scope is invalid");
        }
        let comment = Comment {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            source_text: normalize_source(source_text)?,
            note: normalize_note(note)?,
            created_at_ns: now_ns(),
        };
        let directory = self.collection_dir(scope)?;
        ensure_private_dir(&directory)?;
        atomic_json(&directory.join(format!("{}.json", comment.id)), &comment)?;
        Ok(comment)
    }

    pub fn list_comments(&self, scope: &str) -> Result<Vec<Comment>> {
        let directory = self.collection_dir(scope)?;
        ensure_private_dir(&directory)?;
        let mut comments = Vec::new();
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
                continue;
            };
            if !valid_id(id) {
                continue;
            }
            let comment: Comment = read_json(&entry.path())?;
            validate_model(&comment.id, id, comment.schema_version)?;
            comments.push(comment);
        }
        comments.sort_by(|left, right| {
            (left.created_at_ns, &left.id).cmp(&(right.created_at_ns, &right.id))
        });
        Ok(comments)
    }

    pub fn delete_comments(&self, scope: &str, ids: &[String]) -> Result<()> {
        let directory = self.collection_dir(scope)?;
        for id in ids {
            validate_id(id)?;
            remove_private_file(&directory.join(format!("{id}.json")))?;
        }
        Ok(())
    }

    pub fn comment_path(&self, scope: &str, id: &str) -> PathBuf {
        self.collection_dir(scope)
            .unwrap_or_else(|_| self.root.join("invalid"))
            .join(format!("{id}.json"))
    }

    pub fn create_review(
        &self,
        pane_id: &str,
        scope: &str,
        comment_ids: Vec<String>,
        markdown: &str,
    ) -> Result<ReviewSession> {
        if pane_id.trim().is_empty() || !valid_scope(scope) || comment_ids.is_empty() {
            bail!("review context is invalid");
        }
        for id in &comment_ids {
            validate_id(id)?;
        }
        let review = ReviewSession {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            pane_id: pane_id.to_owned(),
            scope: scope.to_owned(),
            comment_ids,
            created_at_ms: now_ms(),
        };
        atomic_json(&self.review_json_path(&review.id)?, &review)?;
        atomic_bytes(&self.review_markdown_path(&review.id)?, markdown.as_bytes())?;
        Ok(review)
    }

    pub fn load_review(&self, id: &str) -> Result<ReviewSession> {
        let review: ReviewSession = read_json(&self.review_json_path(id)?)?;
        validate_model(&review.id, id, review.schema_version)?;
        Ok(review)
    }

    pub fn review_markdown_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("reviews").join(format!("{id}.md")))
    }

    pub fn read_review_markdown(&self, id: &str) -> Result<String> {
        let path = self.review_markdown_path(id)?;
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
    }

    pub fn confirm_review(&self, id: &str) -> Result<()> {
        atomic_bytes(&self.review_marker_path(id)?, b"confirmed\n")
    }

    pub fn review_is_confirmed(&self, id: &str) -> Result<bool> {
        Ok(self.review_marker_path(id)?.is_file())
    }

    pub fn delete_review(&self, id: &str) -> Result<()> {
        for path in [
            self.review_json_path(id)?,
            self.review_markdown_path(id)?,
            self.review_marker_path(id)?,
        ] {
            remove_private_file(&path)?;
        }
        Ok(())
    }

    pub fn cleanup_transients(&self) -> Result<usize> {
        self.cleanup_transients_at(now_ms())
    }

    fn cleanup_transients_at(&self, now: u64) -> Result<usize> {
        let mut removed = 0;
        for entry in fs::read_dir(self.root.join("drafts"))? {
            let entry = entry?;
            let Some(id) = json_id(&entry.path()) else {
                continue;
            };
            let draft: Draft = read_json(&entry.path())?;
            if now.saturating_sub(draft.created_at_ms) > TRANSIENT_TTL_MS {
                self.delete_draft(&id)?;
                removed += 1;
            }
        }
        for entry in fs::read_dir(self.root.join("reviews"))? {
            let entry = entry?;
            let Some(id) = json_id(&entry.path()) else {
                continue;
            };
            let review: ReviewSession = read_json(&entry.path())?;
            if now.saturating_sub(review.created_at_ms) > TRANSIENT_TTL_MS {
                self.delete_review(&id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn draft_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("drafts").join(format!("{id}.json")))
    }

    fn collection_dir(&self, scope: &str) -> Result<PathBuf> {
        if !valid_scope(scope) {
            bail!("comment scope is invalid");
        }
        Ok(self.root.join("collections").join(scope))
    }

    fn review_json_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("reviews").join(format!("{id}.json")))
    }

    fn review_marker_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("reviews").join(format!("{id}.confirmed")))
    }
}

pub fn scope_id(session_identity: &str, pane_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_identity.as_bytes());
    hasher.update([0]);
    hasher.update(pane_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("refusing symbolic-link state directory {}", path.display())
        }
        Ok(metadata) if !metadata.is_dir() => bail!("{} is not a directory", path.display()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .with_context(|| format!("failed to create {}", path.display()))?;
        }
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to secure {}", path.display()))
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    atomic_bytes(path, &bytes)
}

fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if path
        .parent()
        .is_none_or(|parent| fs::symlink_metadata(parent).is_err())
    {
        bail!("state parent directory is unavailable");
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing symbolic-link state file {}", path.display());
    }
    let parent = path.parent().context("state file has no parent")?;
    ensure_private_dir(parent)?;
    let temporary = parent.join(format!(".{}.tmp", new_id()));
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result.with_context(|| format!("failed to write {}", path.display()))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing symbolic-link state file {}", path.display());
    }
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("invalid state file {}", path.display()))
}

fn remove_private_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("refusing symbolic-link state file {}", path.display())
        }
        Ok(_) => {
            fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn validate_model(actual_id: &str, expected_id: &str, schema_version: u32) -> Result<()> {
    if actual_id != expected_id || schema_version != SCHEMA_VERSION {
        bail!("state file has an unsupported schema or identity");
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if !valid_id(id) {
        bail!("state identifier is invalid");
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn valid_scope(scope: &str) -> bool {
    scope.len() == 64 && scope.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn json_id(path: &Path) -> Option<String> {
    path.file_name()?
        .to_str()?
        .strip_suffix(".json")
        .filter(|id| valid_id(id))
        .map(str::to_owned)
}

fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn expired_transients_are_removed_but_comments_remain() {
        let temp = tempdir().unwrap();
        let store = Store::open(temp.path().join("state")).unwrap();
        let scope = scope_id("socket", "w1:p1");
        let old = now_ms().saturating_sub(TRANSIENT_TTL_MS + 1);
        let draft = store
            .create_draft_at("source", "w1:p1", &scope, old)
            .unwrap();
        store.add_comment(&scope, "source", "note").unwrap();

        assert_eq!(store.cleanup_transients_at(now_ms()).unwrap(), 1);
        assert!(store.load_draft(&draft.id).is_err());
        assert_eq!(store.list_comments(&scope).unwrap().len(), 1);
    }
}
