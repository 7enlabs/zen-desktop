use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::events::{DesktopEvent, DesktopEventSink};

pub const TELEMETRY_READY_EVENT: &str = "telemetry_guard_ready";

pub const ZENCODE_OWNED_HOSTS: &[&str] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    BlockedHost { host: String, allowed: String },

    InvalidBaseUrl { url: String, reason: &'static str },
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelemetryError::BlockedHost { host, allowed } => write!(
                f,
                "ZenCode blocked network access to '{host}': only your configured model endpoint '{allowed}' may be contacted. No telemetry is ever sent."
            ),
            TelemetryError::InvalidBaseUrl { url, reason } => {
                write!(f, "invalid model base URL '{url}': {reason}")
            }
        }
    }
}

impl std::error::Error for TelemetryError {}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?;

    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct TelemetryGuard {
    allowed_host: String,
    base_url: String,
}

impl TelemetryGuard {
    pub fn from_base_url(base_url: &str) -> Result<Self, TelemetryError> {
        let url = base_url.trim();
        if url.is_empty() {
            return Err(TelemetryError::InvalidBaseUrl {
                url: base_url.to_string(),
                reason: "base URL is empty",
            });
        }
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(TelemetryError::InvalidBaseUrl {
                url: base_url.to_string(),
                reason: "only http:// and https:// URLs are allowed",
            });
        }
        match host_of(url) {
            Some(host) => Ok(Self {
                allowed_host: host,
                base_url: url.to_string(),
            }),
            None => Err(TelemetryError::InvalidBaseUrl {
                url: base_url.to_string(),
                reason: "could not parse a host from the URL",
            }),
        }
    }

    pub fn allowed_host(&self) -> &str {
        &self.allowed_host
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn allows(&self, host: &str) -> bool {
        host.trim().to_ascii_lowercase() == self.allowed_host
    }

    pub fn check_url(&self, url: &str) -> Result<(), TelemetryError> {
        match host_of(url) {
            Some(host) if self.allows(&host) => Ok(()),
            Some(host) => Err(TelemetryError::BlockedHost {
                host,
                allowed: self.allowed_host.clone(),
            }),
            None => Err(TelemetryError::BlockedHost {
                host: url.to_string(),
                allowed: self.allowed_host.clone(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPurpose {
    ModelChat,

    UserOptIn,
}

impl fmt::Display for NetworkPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkPurpose::ModelChat => write!(f, "model chat"),
            NetworkPurpose::UserOptIn => write!(f, "user opt-in"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub ts_ms: u64,
    pub host: String,
    pub path: String,
    pub purpose: NetworkPurpose,
    pub bytes_up: u64,
    pub bytes_down: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkLog {
    entries: Arc<Mutex<Vec<NetworkEntry>>>,
}

impl NetworkLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &self,
        host: impl Into<String>,
        path: impl Into<String>,
        purpose: NetworkPurpose,
        bytes_up: u64,
        bytes_down: u64,
    ) {
        let entry = NetworkEntry {
            ts_ms: now_ms(),
            host: host.into(),
            path: path.into(),
            purpose,
            bytes_up,
            bytes_down,
        };
        self.entries.lock().expect("network log mutex").push(entry);
    }

    pub fn entries(&self) -> Vec<NetworkEntry> {
        self.entries.lock().expect("network log mutex").clone()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("network log mutex").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn distinct_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self
            .entries()
            .into_iter()
            .map(|e| e.host)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        hosts.sort();
        hosts
    }

    pub fn total_bytes(&self) -> (u64, u64) {
        self.entries().iter().fold((0, 0), |(up, down), e| {
            (up + e.bytes_up, down + e.bytes_down)
        })
    }

    pub fn clear(&self) {
        self.entries.lock().expect("network log mutex").clear();
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let mut out = String::new();
        for e in self.entries() {
            out.push_str(&serde_json::to_string(&e)?);
            out.push('\n');
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, out)?;
        Ok(())
    }

    pub fn load_from(path: &Path) -> Self {
        let log = Self::new();
        let Ok(text) = fs::read_to_string(path) else {
            return log;
        };
        {
            let mut guard = log.entries.lock().expect("network log mutex");
            for line in text.lines() {
                if let Ok(entry) = serde_json::from_str::<NetworkEntry>(line) {
                    guard.push(entry);
                }
            }
        }
        log
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryReport {
    pub hosts_contacted: Vec<String>,

    pub zencode_hosts_contacted: Vec<String>,
    pub entry_count: usize,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,

    pub verdict: String,
}

impl TelemetryReport {
    pub fn is_clean(&self) -> bool {
        self.zencode_hosts_contacted.is_empty()
    }
}

pub struct ZeroTelemetryProof;

impl ZeroTelemetryProof {
    pub fn generate(log: &NetworkLog) -> TelemetryReport {
        let hosts = log.distinct_hosts();
        let zencode_hosts_contacted: Vec<String> = hosts
            .iter()
            .filter(|h| {
                ZENCODE_OWNED_HOSTS
                    .iter()
                    .any(|owned| h.eq_ignore_ascii_case(owned))
            })
            .cloned()
            .collect();
        let (up, down) = log.total_bytes();
        let verdict = if zencode_hosts_contacted.is_empty() {
            format!(
                "Clean: {} outbound request(s) to {} host(s) ({}), none owned by ZenCode. No telemetry was sent.",
                log.len(),
                hosts.len(),
                hosts.join(", ")
            )
        } else {
            format!(
                "VIOLATION: contacted ZenCode-owned host(s): {}. This must never happen - please report it.",
                zencode_hosts_contacted.join(", ")
            )
        };
        TelemetryReport {
            hosts_contacted: hosts,
            zencode_hosts_contacted,
            entry_count: log.len(),
            total_bytes_up: up,
            total_bytes_down: down,
            verdict,
        }
    }
}

pub fn emit_telemetry_ready(sink: &Arc<dyn DesktopEventSink>, guard: &TelemetryGuard) {
    sink.emit_desktop(DesktopEvent::SystemMessage {
        event: TELEMETRY_READY_EVENT.to_string(),
        text: format!(
            "zero-telemetry guard armed: the only host this run may contact is '{}'. No analytics, no crash reports, no phone-home.",
            guard.allowed_host()
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_accepts_configured_host() {
        let g = TelemetryGuard::from_base_url("https://ai-gateway.kilo.ai/api/v1").unwrap();
        assert_eq!(g.allowed_host(), "ai-gateway.kilo.ai");
        assert!(g.allows("ai-gateway.kilo.ai"));
        assert!(g.allows("AI-GATEWAY.KILO.AI"));
        assert!(g
            .check_url("https://ai-gateway.kilo.ai/api/v1/chat/completions")
            .is_ok());
    }

    #[test]
    fn guard_blocks_foreign_host() {
        let g = TelemetryGuard::from_base_url("https://ai-gateway.kilo.ai/api/v1").unwrap();
        assert!(!g.allows("telemetry.zencode.example"));
        let err = g
            .check_url("https://telemetry.zencode.example/v1/ping")
            .unwrap_err();
        assert!(matches!(err, TelemetryError::BlockedHost { .. }));
        assert!(err.to_string().contains("telemetry.zencode.example"));
    }

    #[test]
    fn guard_rejects_bad_base_urls() {
        assert!(matches!(
            TelemetryGuard::from_base_url(""),
            Err(TelemetryError::InvalidBaseUrl { .. })
        ));
        assert!(matches!(
            TelemetryGuard::from_base_url("ftp://example.com/x"),
            Err(TelemetryError::InvalidBaseUrl { .. })
        ));
        assert!(matches!(
            TelemetryGuard::from_base_url("https:///no-host"),
            Err(TelemetryError::InvalidBaseUrl { .. })
        ));
        assert!(matches!(
            TelemetryGuard::from_base_url("not a url"),
            Err(TelemetryError::InvalidBaseUrl { .. })
        ));
    }

    #[test]
    fn guard_strips_port_and_userinfo() {
        let g = TelemetryGuard::from_base_url("http://user:pw@localhost:11434/v1").unwrap();
        assert_eq!(g.allowed_host(), "localhost");
        assert!(g
            .check_url("http://localhost:11434/v1/chat/completions")
            .is_ok());
    }

    #[test]
    fn log_records_and_clears() {
        let log = NetworkLog::new();
        assert!(log.is_empty());
        log.record("a.example", "/x", NetworkPurpose::ModelChat, 100, 50);
        log.record("b.example", "/y", NetworkPurpose::UserOptIn, 10, 5);
        assert_eq!(log.len(), 2);
        assert_eq!(log.distinct_hosts(), vec!["a.example", "b.example"]);
        assert_eq!(log.total_bytes(), (110, 55));
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn log_save_load_roundtrip() {
        let dir = std::env::temp_dir().join("zencode-telemetry-test");
        let path = dir.join("netlog.ndjson");
        let _ = fs::remove_dir_all(&dir);
        let log = NetworkLog::new();
        log.record("a.example", "/x", NetworkPurpose::ModelChat, 100, 50);
        log.save_to(&path).unwrap();

        fs::write(
            &path,
            format!("{}\nNOT-JSON\n", fs::read_to_string(&path).unwrap()),
        )
        .unwrap();
        let loaded = NetworkLog::load_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.entries()[0].host, "a.example");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_load_missing_file_is_empty() {
        let loaded = NetworkLog::load_from(Path::new("/nonexistent-dir-xyz/netlog.ndjson"));
        assert!(loaded.is_empty());
    }

    #[test]
    fn proof_clean_when_only_configured_host() {
        let log = NetworkLog::new();
        log.record(
            "ai-gateway.kilo.ai",
            "/chat/completions",
            NetworkPurpose::ModelChat,
            100,
            50,
        );
        let report = ZeroTelemetryProof::generate(&log);
        assert!(report.is_clean());
        assert!(report.zencode_hosts_contacted.is_empty());
        assert_eq!(report.hosts_contacted, vec!["ai-gateway.kilo.ai"]);
        assert!(report.verdict.starts_with("Clean"));
    }

    #[test]
    fn proof_clean_on_empty_log() {
        let report = ZeroTelemetryProof::generate(&NetworkLog::new());
        assert!(report.is_clean());
        assert_eq!(report.entry_count, 0);
    }

    #[test]
    fn zencode_owned_hosts_is_empty_by_design() {
        assert!(ZENCODE_OWNED_HOSTS.is_empty());
    }
}
