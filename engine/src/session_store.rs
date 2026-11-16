use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::events::{DesktopEvent, Envelope};

const INDEX_VERSION: u32 = 1;
const INDEX_FILE: &str = "index.json";

const PREVIEW_CHARS: usize = 120;

const TITLE_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: u64,
    pub model: String,
    pub cwd: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    sessions: Vec<SessionMeta>,
}

pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn with_default_root() -> anyhow::Result<Self> {
        Ok(Self::new(Self::default_root()?))
    }

    pub fn default_root() -> anyhow::Result<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|h| PathBuf::from(h).join(".zen").join("sessions"))
            .ok_or_else(|| {
                anyhow::anyhow!("cannot determine home directory (HOME/USERPROFILE unset)")
            })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn new_session_id() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = now_secs();
        let mut h = DefaultHasher::new();
        (
            now,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        )
            .hash(&mut h);

        let hex = format!("{:012x}", h.finish() & 0xffff_ffff_ffff);
        format!("{}-{hex}", &unix_to_rfc3339(now)[..10])
    }

    fn session_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.ndjson"))
    }

    pub fn save_session(
        &self,
        id: &str,
        events: &[Envelope],
        title: Option<&str>,
        model: &str,
        cwd: &Path,
    ) -> anyhow::Result<()> {
        validate_session_id(id)?;
        fs::create_dir_all(&self.root)?;

        let mut ndjson = String::new();
        for e in events {
            ndjson.push_str(&serde_json::to_string(e)?);
            ndjson.push('\n');
        }
        fs::write(self.session_path(id), ndjson)?;

        let mut idx = self.load_index()?;
        let now = unix_to_rfc3339(now_secs());
        let first_text = first_assistant_text(events);
        let preview = first_text
            .as_deref()
            .map(|t| truncate_chars(t, PREVIEW_CHARS))
            .unwrap_or_default();
        match idx.sessions.iter_mut().find(|s| s.id == id) {
            Some(meta) => {
                meta.updated_at = now;
                meta.preview = preview;
                meta.message_count = message_count(events);
                meta.model = model.to_string();
                meta.cwd = cwd.display().to_string();
                if let Some(t) = title {
                    meta.title = t.to_string();
                }
            }
            None => {
                let derived = first_text.map(|t| truncate_chars(&t, TITLE_CHARS));
                idx.sessions.push(SessionMeta {
                    id: id.to_string(),
                    title: title
                        .map(str::to_string)
                        .or(derived)
                        .unwrap_or_else(|| id.to_string()),
                    preview,
                    created_at: now.clone(),
                    updated_at: now,
                    message_count: message_count(events),
                    model: model.to_string(),
                    cwd: cwd.display().to_string(),
                    parent_id: None,
                });
            }
        }
        self.write_index(&idx)
    }

    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionMeta>> {
        let mut idx = self.load_index()?;
        sort_newest_first(&mut idx.sessions);
        Ok(idx.sessions)
    }

    pub fn search_sessions(&self, query: &str) -> anyhow::Result<Vec<SessionMeta>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.list_sessions();
        }
        let mut hits: Vec<(u8, SessionMeta)> = self
            .list_sessions()?
            .into_iter()
            .filter_map(|m| {
                if m.title.to_lowercase().contains(&q) {
                    Some((0, m))
                } else if m.preview.to_lowercase().contains(&q) {
                    Some((1, m))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| b.1.updated_at.cmp(&a.1.updated_at))
        });
        Ok(hits.into_iter().map(|(_, m)| m).collect())
    }

    pub fn rename_session(&self, id: &str, new_title: &str) -> anyhow::Result<()> {
        validate_session_id(id)?;
        let new_title = new_title.trim();
        if new_title.is_empty() {
            anyhow::bail!("new title must not be empty");
        }
        fs::create_dir_all(&self.root)?;
        let mut idx = self.load_index()?;
        let Some(meta) = idx.sessions.iter_mut().find(|s| s.id == id) else {
            anyhow::bail!("unknown session id: {id}");
        };
        meta.title = new_title.to_string();
        meta.updated_at = unix_to_rfc3339(now_secs());
        self.write_index(&idx)
    }

    fn load_index(&self) -> anyhow::Result<IndexFile> {
        let path = self.root.join(INDEX_FILE);
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(idx) = serde_json::from_str::<IndexFile>(&text) {
                if idx.version == INDEX_VERSION {
                    return self.prune_and_persist(idx);
                }
            }
        }
        self.rebuild_index()
    }

    fn prune_and_persist(&self, mut idx: IndexFile) -> anyhow::Result<IndexFile> {
        let before = idx.sessions.len();
        idx.sessions.retain(|s| self.session_path(&s.id).is_file());
        if idx.sessions.len() != before {
            self.write_index(&idx)?;
        }
        Ok(idx)
    }

    fn rebuild_index(&self) -> anyhow::Result<IndexFile> {
        fs::create_dir_all(&self.root)?;
        let mut sessions = vec![];
        let entries = fs::read_dir(&self.root)?.filter_map(Result::ok);
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ndjson") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if validate_session_id(stem).is_err() {
                continue;
            }
            let events = read_envelopes_best_effort(&path);
            let first_text = first_assistant_text(&events);
            let ts = unix_to_rfc3339(file_mtime_secs(&path));
            sessions.push(SessionMeta {
                id: stem.to_string(),
                title: first_text
                    .as_deref()
                    .map(|t| truncate_chars(t, TITLE_CHARS))
                    .unwrap_or_else(|| stem.to_string()),
                preview: first_text
                    .as_deref()
                    .map(|t| truncate_chars(t, PREVIEW_CHARS))
                    .unwrap_or_default(),
                created_at: ts.clone(),
                updated_at: ts,
                message_count: message_count(&events),
                model: String::new(),
                cwd: String::new(),
                parent_id: None,
            });
        }
        sort_newest_first(&mut sessions);
        let idx = IndexFile {
            version: INDEX_VERSION,
            sessions,
        };
        self.write_index(&idx)?;
        Ok(idx)
    }

    fn write_index(&self, idx: &IndexFile) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)?;
        let text = serde_json::to_string_pretty(idx)?;
        let tmp = self.root.join(format!("{INDEX_FILE}.tmp"));
        fs::write(&tmp, text)?;
        fs::rename(&tmp, self.root.join(INDEX_FILE))?;
        Ok(())
    }
}

fn validate_session_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 128 {
        anyhow::bail!("invalid session id: empty or longer than 128 chars");
    }
    if id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Ok(())
    } else {
        anyhow::bail!("invalid session id {id:?}: only [A-Za-z0-9._-] allowed");
    }
}

fn sort_newest_first(sessions: &mut [SessionMeta]) {
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
    });
}

fn read_envelopes_best_effort(path: &Path) -> Vec<Envelope> {
    let Ok(text) = fs::read_to_string(path) else {
        return vec![];
    };
    match crate::events::parse_ndjson(&text) {
        Ok(evs) => evs,
        Err(_) => text
            .lines()
            .filter_map(|l| serde_json::from_str(l.trim()).ok())
            .collect(),
    }
}

fn first_assistant_text(events: &[Envelope]) -> Option<String> {
    events.iter().find_map(|e| match &e.event {
        DesktopEvent::AssistantMessage { text: Some(t), .. } if !t.trim().is_empty() => {
            Some(t.trim().to_string())
        }
        _ => None,
    })
}

fn message_count(events: &[Envelope]) -> u64 {
    events
        .iter()
        .filter(|e| {
            matches!(
                e.event,
                DesktopEvent::AssistantMessage { .. } | DesktopEvent::ToolResult { .. }
            )
        })
        .count() as u64
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_else(now_secs)
}

fn unix_to_rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };
    format!("{year:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{DesktopEvent, Envelope, SCHEMA_VERSION};

    fn tmp_root(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("zen-session-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn assistant(seq: u64, text: &str) -> Envelope {
        Envelope {
            v: SCHEMA_VERSION,
            seq,
            event: DesktopEvent::AssistantMessage {
                text: Some(text.to_string()),
                tool_calls: vec![],
                parent_tool_use_id: None,
            },
        }
    }

    fn tool_result(seq: u64) -> Envelope {
        Envelope {
            v: SCHEMA_VERSION,
            seq,
            event: DesktopEvent::ToolResult {
                tool_use_id: "t1".into(),
                content: "ok".into(),
                is_error: false,
                parent_tool_use_id: None,
            },
        }
    }

    fn store(name: &str) -> (SessionStore, PathBuf) {
        let root = tmp_root(name);
        (SessionStore::new(&root), root)
    }

    #[test]
    fn save_list_roundtrip() {
        let (store, root) = store("roundtrip");
        let events = vec![
            assistant(0, "Fix the login redirect loop on /dashboard for good"),
            tool_result(1),
            assistant(2, "Done."),
        ];
        store
            .save_session(
                "2026-10-17-aaa",
                &events,
                None,
                "m:free",
                Path::new("/tmp/proj"),
            )
            .unwrap();

        let raw = fs::read_to_string(root.join("2026-10-17-aaa.ndjson")).unwrap();
        let parsed = crate::events::parse_ndjson(&raw).unwrap();
        assert_eq!(parsed, events);

        let list = store.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        let m = &list[0];
        assert_eq!(m.id, "2026-10-17-aaa");
        assert_eq!(
            m.title,
            "Fix the login redirect loop on /dashboard for good"
        );
        assert_eq!(
            m.preview,
            "Fix the login redirect loop on /dashboard for good"
        );
        assert_eq!(m.message_count, 3);
        assert_eq!(m.model, "m:free");
        assert_eq!(m.cwd, "/tmp/proj");
        assert_eq!(m.parent_id, None);
        assert!(m.created_at <= m.updated_at);
    }

    #[test]
    fn empty_store_lists_empty() {
        let (store, _) = store("empty");
        assert!(store.list_sessions().unwrap().is_empty());
        assert!(store.search_sessions("anything").unwrap().is_empty());
    }

    #[test]
    fn search_ranks_title_above_preview() {
        let (store, _) = store("search");
        store
            .save_session(
                "s-title",
                &[assistant(0, "unrelated text here")],
                Some("Fix the login bug"),
                "m",
                Path::new("/x"),
            )
            .unwrap();
        store
            .save_session(
                "s-preview",
                &[assistant(0, "please fix the login flow today")],
                Some("Unrelated title"),
                "m",
                Path::new("/x"),
            )
            .unwrap();

        let hits = store.search_sessions("LOGIN").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "s-title");
        assert_eq!(hits[1].id, "s-preview");

        assert!(store.search_sessions("no-such-thing").unwrap().is_empty());

        assert_eq!(store.search_sessions("  ").unwrap().len(), 2);
    }

    #[test]
    fn rename_updates_index_only() {
        let (store, root) = store("rename");
        let events = vec![assistant(0, "original text")];
        store
            .save_session("s1", &events, None, "m", Path::new("/x"))
            .unwrap();
        let before = fs::read(root.join("s1.ndjson")).unwrap();

        store.rename_session("s1", "New title").unwrap();

        assert_eq!(fs::read(root.join("s1.ndjson")).unwrap(), before);
        let m = &store.list_sessions().unwrap()[0];
        assert_eq!(m.title, "New title");

        assert_eq!(m.preview, "original text");
    }

    #[test]
    fn rename_unknown_id_errors() {
        let (store, _) = store("rename-unknown");
        assert!(store.rename_session("nope", "t").is_err());
        assert!(store.rename_session("s1", "   ").is_err());
    }

    #[test]
    fn rebuild_from_files_when_index_missing_or_corrupt() {
        let (store, root) = store("rebuild");
        store
            .save_session(
                "2026-10-17-r1",
                &[assistant(0, "rebuild me please")],
                None,
                "m",
                Path::new("/x"),
            )
            .unwrap();

        fs::remove_file(root.join("index.json")).unwrap();
        let list = store.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "2026-10-17-r1");
        assert_eq!(list[0].title, "rebuild me please");

        fs::write(root.join("index.json"), "{not json").unwrap();
        let list = store.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "2026-10-17-r1");
    }

    #[test]
    fn rebuild_prunes_orphan_index_entries() {
        let (store, root) = store("prune");
        store
            .save_session("real", &[assistant(0, "hi")], None, "m", Path::new("/x"))
            .unwrap();

        fs::remove_file(root.join("real.ndjson")).unwrap();
        let list = store.list_sessions().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn invalid_session_id_rejected() {
        let (store, _) = store("bad-id");
        let events = vec![assistant(0, "hi")];
        for bad in ["", "../evil", "a/b", "with space", "semi;colon"] {
            assert!(
                store
                    .save_session(bad, &events, None, "m", Path::new("/x"))
                    .is_err(),
                "id {bad:?} should be rejected"
            );
        }
        assert!(store.rename_session("../../x", "t").is_err());
    }

    #[test]
    fn new_session_id_format_and_uniqueness() {
        let a = SessionStore::new_session_id();
        let b = SessionStore::new_session_id();
        assert_ne!(a, b);
        for id in [a, b] {
            assert!(validate_session_id(&id).is_ok(), "bad id: {id}");
            let parts: Vec<&str> = id.split('-').collect();
            assert_eq!(parts.len(), 4);
            assert_eq!(parts[3].len(), 12);
            assert!(parts[3].chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn rfc3339_known_values() {
        assert_eq!(unix_to_rfc3339(0), "1970-01-01T00:00:00Z");

        assert_eq!(unix_to_rfc3339(1_792_195_200), "2026-10-17T00:00:00Z");

        assert_eq!(unix_to_rfc3339(1_709_210_096), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn title_preview_truncation() {
        let (store, _) = store("trunc");
        let long: String = "x".repeat(500);
        store
            .save_session("t1", &[assistant(0, &long)], None, "m", Path::new("/x"))
            .unwrap();
        let m = &store.list_sessions().unwrap()[0];
        assert_eq!(m.title.chars().count(), TITLE_CHARS);
        assert_eq!(m.preview.chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn unicode_truncation_is_char_safe() {
        let (store, _) = store("unicode");
        let text = "héllo wörld - ".repeat(50);
        store
            .save_session("u1", &[assistant(0, &text)], None, "m", Path::new("/x"))
            .unwrap();
        let m = &store.list_sessions().unwrap()[0];
        assert_eq!(m.title.chars().count(), TITLE_CHARS);
        assert!(m.title.is_char_boundary(m.title.len()));
    }
}
