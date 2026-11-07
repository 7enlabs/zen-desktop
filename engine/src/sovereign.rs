use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const OLLAMA_LOOPBACK_URL: &str = "http://localhost:11434/v1";

pub const OLLAMA_DEFAULT_PORT: u16 = 11434;

pub const POLICY_VERSION: &str = "sovereign-policy-v1";

pub const ATTESTATION_SCHEME: &str = "hash-chain-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressDecision {
    Allow,

    Block { url: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SovereignError {
    BlockedUrl { url: String, reason: String },

    NonSovereignConfig { detail: String },

    PreflightBlocked { findings: Vec<String> },

    OllamaUnreachable { endpoint: String },
}

impl fmt::Display for SovereignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SovereignError::BlockedUrl { url, reason } => write!(
                f,
                "Sovereign Mode blocked network egress to '{url}': {reason}. \
                 Only localhost traffic is allowed while Sovereign Mode is on."
            ),
            SovereignError::NonSovereignConfig { detail } => write!(
                f,
                "Not a sovereign configuration: {detail}. Sovereign Mode pins \
                 inference to Ollama on localhost with no API key."
            ),
            SovereignError::PreflightBlocked { findings } => write!(
                f,
                "Sovereign Mode refused to engage until preflight findings are \
                 resolved: {}",
                findings.join("; ")
            ),
            SovereignError::OllamaUnreachable { endpoint } => write!(
                f,
                "Sovereign Mode needs a local Ollama instance, but {endpoint} \
                 is not reachable. Start Ollama first (`ollama serve`), then \
                 enable Sovereign Mode."
            ),
        }
    }
}

impl std::error::Error for SovereignError {}

fn split_host_port(url: &str) -> Option<(String, Option<u16>)> {
    let no_frag = url.split('#').next().unwrap_or(url);
    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
    let rest = no_query.split("://").nth(1)?;
    let authority = rest.split('/').next().unwrap_or(rest);

    let hostport = authority.rsplit('@').next().unwrap_or(authority);

    if let Some(stripped) = hostport.strip_prefix('[') {
        let end = stripped.find(']')?;
        let host = format!("[{}]", &stripped[..end]);
        let port = stripped[end + 1..].strip_prefix(':')?.parse::<u16>().ok();
        return Some((host, port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port_str)) if port_str.parse::<u16>().is_ok() => {
            Some((host.to_string(), port_str.parse().ok()))
        }
        _ => Some((hostport.to_string(), None)),
    }
}

pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    if h == "localhost" || h == "0.0.0.0" {
        return true;
    }

    let bare = h
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&h);
    if bare == "::1" {
        return true;
    }
    if let Ok(IpAddr::V4(v4)) = bare.parse::<IpAddr>() {
        return v4.octets()[0] == 127;
    }
    false
}

pub fn check_url_sovereign(url: &str) -> EgressDecision {
    match split_host_port(url) {
        Some((host, _)) if is_loopback_host(&host) => EgressDecision::Allow,
        Some((host, _)) => EgressDecision::Block {
            url: url.to_string(),
            reason: format!(
                "host '{host}' is not loopback; Sovereign Mode permits only localhost traffic"
            ),
        },
        None => EgressDecision::Block {
            url: url.to_string(),
            reason: "URL has no parseable host; refusing by default".to_string(),
        },
    }
}

pub fn validate_llm_config(base_url: &str, api_key: Option<&str>) -> Result<(), SovereignError> {
    if let Some(k) = api_key {
        if !k.trim().is_empty() {
            return Err(SovereignError::NonSovereignConfig {
                detail:
                    "an API key is configured; sovereign inference uses local Ollama with no key"
                        .to_string(),
            });
        }
    }
    match check_url_sovereign(base_url) {
        EgressDecision::Allow => Ok(()),
        EgressDecision::Block { reason, .. } => Err(SovereignError::NonSovereignConfig {
            detail: format!("LLM endpoint '{base_url}' is not local: {reason}"),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightFinding {
    pub kind: String,

    pub detail: String,

    pub acknowledgable: bool,
}

pub fn preflight_check(
    base_url: &str,
    api_key: Option<&str>,
    extra_urls: &[String],
    probe_ollama: bool,
) -> Vec<PreflightFinding> {
    let mut findings = Vec::new();

    match check_url_sovereign(base_url) {
        EgressDecision::Allow => {}
        EgressDecision::Block { reason, .. } => findings.push(PreflightFinding {
            kind: "remote-endpoint".to_string(),
            detail: format!("LLM endpoint '{base_url}' would leave the machine: {reason}"),
            acknowledgable: false,
        }),
    }

    if let Some(k) = api_key {
        if !k.trim().is_empty() {
            findings.push(PreflightFinding {
                kind: "api-key-present".to_string(),
                detail: "An API key is configured. Sovereign Mode uses local Ollama \
                         with no key; the key will be ignored while the mode is on, \
                         which may surprise you if you expected remote inference."
                    .to_string(),
                acknowledgable: true,
            });
        }
    }

    for url in extra_urls {
        if let EgressDecision::Block { reason, .. } = check_url_sovereign(url) {
            findings.push(PreflightFinding {
                kind: "remote-endpoint".to_string(),
                detail: format!("Configured URL '{url}' would leave the machine: {reason}"),
                acknowledgable: false,
            });
        }
    }

    if probe_ollama && !probe_ollama_running(OLLAMA_DEFAULT_PORT) {
        findings.push(PreflightFinding {
            kind: "ollama-unreachable".to_string(),
            detail: format!(
                "No Ollama instance is listening on 127.0.0.1:{OLLAMA_DEFAULT_PORT}. \
                 Start it (`ollama serve`) before enabling Sovereign Mode."
            ),
            acknowledgable: false,
        });
    }

    findings
}

pub fn probe_ollama_running(port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationEntry {
    pub seq: u64,

    pub ts: String,

    pub kind: String,

    pub detail: String,

    pub prev_hash: String,

    pub hash: String,
}

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        m,
        d,
        sod / 3_600,
        (sod % 3_600) / 60,
        sod % 60
    )
}

fn entry_hash(seq: u64, ts: &str, kind: &str, detail: &str, prev_hash: &str) -> String {
    sha256_hex(&format!("{seq}|{ts}|{kind}|{detail}|{prev_hash}"))
}

#[derive(Debug, Clone)]
pub struct AttestationLog {
    entries: Vec<AttestationEntry>,
}

impl AttestationLog {
    pub fn new(session_id: &str) -> Self {
        let genesis_detail = format!(
            "session {session_id} started under {POLICY_VERSION}; inference pinned to {OLLAMA_LOOPBACK_URL}"
        );
        let entry = AttestationEntry {
            seq: 0,
            ts: now_rfc3339(),
            kind: "genesis".to_string(),
            detail: genesis_detail,
            prev_hash: "GENESIS".to_string(),
            hash: String::new(),
        };
        let hash = entry_hash(
            entry.seq,
            &entry.ts,
            &entry.kind,
            &entry.detail,
            &entry.prev_hash,
        );
        let mut log = Self {
            entries: Vec::new(),
        };
        log.entries.push(AttestationEntry { hash, ..entry });
        log
    }

    pub fn record(&mut self, kind: &str, detail: &str) {
        let prev_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| "GENESIS".to_string());
        let seq = self.entries.len() as u64;
        let ts = now_rfc3339();
        let hash = entry_hash(seq, &ts, kind, detail, &prev_hash);
        self.entries.push(AttestationEntry {
            seq,
            ts,
            kind: kind.to_string(),
            detail: detail.to_string(),
            prev_hash,
            hash,
        });
    }

    pub fn entries(&self) -> &[AttestationEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn finalize(&self, session_id: &str) -> Attestation {
        Attestation {
            scheme: ATTESTATION_SCHEME.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            session_id: session_id.to_string(),
            finalized_at: now_rfc3339(),
            entries: self.entries.clone(),
            final_hash: self
                .entries
                .last()
                .map(|e| e.hash.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    pub scheme: String,
    pub policy_version: String,
    pub session_id: String,
    pub finalized_at: String,
    pub entries: Vec<AttestationEntry>,
    pub final_hash: String,
}

pub fn verify_attestation(att: &Attestation) -> bool {
    if att.scheme != ATTESTATION_SCHEME || att.policy_version != POLICY_VERSION {
        return false;
    }
    if att.entries.is_empty() {
        return false;
    }
    let mut prev = "GENESIS".to_string();
    for (i, e) in att.entries.iter().enumerate() {
        if e.seq != i as u64 || e.prev_hash != prev {
            return false;
        }
        let recomputed = entry_hash(e.seq, &e.ts, &e.kind, &e.detail, &e.prev_hash);
        if recomputed != e.hash {
            return false;
        }
        prev = e.hash.clone();
    }
    match att.entries.last() {
        Some(last) => last.hash == att.final_hash,
        None => false,
    }
}

#[derive(Debug)]
pub struct SovereignMode {
    session_id: String,
    ollama_base_url: String,
    log: AttestationLog,
    blocked_egress: u64,
    allowed_egress: u64,
}

impl SovereignMode {
    pub fn enable(session_id: &str) -> Result<Self, SovereignError> {
        Self::enable_with_probe(session_id, OLLAMA_LOOPBACK_URL, true)
    }

    pub fn enable_with_probe(
        session_id: &str,
        ollama_base_url: &str,
        probe: bool,
    ) -> Result<Self, SovereignError> {
        validate_llm_config(ollama_base_url, None)?;

        if probe && !probe_ollama_running(OLLAMA_DEFAULT_PORT) {
            return Err(SovereignError::OllamaUnreachable {
                endpoint: format!("127.0.0.1:{OLLAMA_DEFAULT_PORT}"),
            });
        }
        let mut log = AttestationLog::new(session_id);
        log.record(
            "config-validated",
            &format!("inference endpoint validated as loopback: {ollama_base_url}"),
        );
        Ok(Self {
            session_id: session_id.to_string(),
            ollama_base_url: ollama_base_url.to_string(),
            log,
            blocked_egress: 0,
            allowed_egress: 0,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn ollama_base_url(&self) -> &str {
        &self.ollama_base_url
    }

    pub fn check_egress(&mut self, url: &str) -> Result<(), SovereignError> {
        match check_url_sovereign(url) {
            EgressDecision::Allow => {
                self.allowed_egress += 1;
                Ok(())
            }
            EgressDecision::Block { reason, .. } => {
                self.blocked_egress += 1;
                self.log
                    .record("egress-blocked", &format!("blocked '{url}': {reason}"));
                Err(SovereignError::BlockedUrl {
                    url: url.to_string(),
                    reason,
                })
            }
        }
    }

    pub fn apply_to_config(
        &mut self,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<(String, Option<String>), SovereignError> {
        validate_llm_config(base_url, api_key)?;
        self.log.record(
            "config-validated",
            &format!(
                "task config validated; inference pinned to {}",
                self.ollama_base_url
            ),
        );
        Ok((self.ollama_base_url.clone(), None))
    }

    pub fn blocked_egress_count(&self) -> u64 {
        self.blocked_egress
    }

    pub fn allowed_egress_count(&self) -> u64 {
        self.allowed_egress
    }

    pub fn attestation_entries(&self) -> &[AttestationEntry] {
        self.log.entries()
    }

    pub fn disable(mut self) -> Attestation {
        self.log.record(
            "mode-disabled",
            &format!(
                "sovereign mode disabled; egress allowed={} blocked={}",
                self.allowed_egress, self.blocked_egress
            ),
        );
        self.log.finalize(&self.session_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SovereignStatus {
    pub enabled: bool,
    pub session_id: Option<String>,
    pub ollama_endpoint: String,
    pub ollama_reachable: bool,
    pub allowed_egress: u64,
    pub blocked_egress: u64,
}

impl SovereignStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            session_id: None,
            ollama_endpoint: OLLAMA_LOOPBACK_URL.to_string(),
            ollama_reachable: probe_ollama_running(OLLAMA_DEFAULT_PORT),
            allowed_egress: 0,
            blocked_egress: 0,
        }
    }

    pub fn from_mode(mode: &SovereignMode) -> Self {
        Self {
            enabled: true,
            session_id: Some(mode.session_id().to_string()),
            ollama_endpoint: mode.ollama_base_url().to_string(),
            ollama_reachable: true,
            allowed_egress: mode.allowed_egress_count(),
            blocked_egress: mode.blocked_egress_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn loopback_hosts_allowed() {
        for host in [
            "localhost",
            "LOCALHOST",
            "localhost.",
            "127.0.0.1",
            "127.0.0.2",
            "127.255.255.255",
            "::1",
            "[::1]",
            "0.0.0.0",
        ] {
            assert!(is_loopback_host(host), "expected loopback: {host}");
        }
    }

    #[test]
    fn non_loopback_hosts_rejected() {
        for host in [
            "example.com",
            "8.8.8.8",
            "192.168.1.1",
            "10.0.0.5",
            "172.16.0.1",
            "localhost.example.com",
            "evil-localhost.com",
            "",
        ] {
            assert!(!is_loopback_host(host), "expected non-loopback: {host}");
        }
    }

    #[test]
    fn sovereign_allows_ollama_urls() {
        for url in [
            "http://localhost:11434/v1",
            "http://localhost:11434/api/chat",
            "http://127.0.0.1:11434/v1/chat/completions",
            "http://[::1]:11434/v1",
        ] {
            assert_eq!(check_url_sovereign(url), EgressDecision::Allow, "{url}");
        }
    }

    #[test]
    fn sovereign_blocks_remote_urls() {
        for url in [
            "https://api.openai.com/v1/chat/completions",
            "https://example.com:443/x?key=secret",
            "http://192.168.1.10:11434/v1",
            "https://user:pass@api.example.com/v1", // trufflehog:ignore -- test fixture only, example URL, no real credentials; gitleaks:allow
            "not-a-url",
            "",
        ] {
            match check_url_sovereign(url) {
                EgressDecision::Block { .. } => {}
                EgressDecision::Allow => panic!("should have blocked: {url}"),
            }
        }
    }

    #[test]
    fn validate_accepts_ollama_without_key() {
        assert!(validate_llm_config("http://localhost:11434/v1", None).is_ok());
        assert!(validate_llm_config("http://localhost:11434/v1", Some("  ")).is_ok());
    }

    #[test]
    fn validate_rejects_remote_endpoint() {
        let err = validate_llm_config("https://api.openai.com/v1", None).unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
    }

    #[test]
    fn validate_rejects_any_api_key() {
        let err = validate_llm_config("http://localhost:11434/v1", Some("sk-abc")).unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
    }

    #[test]
    fn preflight_clean_config_has_no_findings() {
        let findings = preflight_check("http://localhost:11434/v1", None, &[], false);
        assert!(findings.is_empty());
    }

    #[test]
    fn preflight_flags_remote_endpoint_and_key() {
        let findings = preflight_check(
            "https://api.openai.com/v1",
            Some("sk-secret"),
            &["https://telemetry.example.com/ping".to_string()],
            false,
        );
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"remote-endpoint"));
        assert!(kinds.contains(&"api-key-present"));

        assert!(findings
            .iter()
            .filter(|f| f.kind == "remote-endpoint")
            .all(|f| !f.acknowledgable));
        assert!(findings
            .iter()
            .filter(|f| f.kind == "api-key-present")
            .all(|f| f.acknowledgable));
    }

    #[test]
    fn attestation_chain_verifies() {
        let mut log = AttestationLog::new("sess-1");
        log.record(
            "egress-allowed",
            "http://localhost:11434/v1/chat/completions",
        );
        log.record("egress-blocked", "blocked 'https://evil.example.com/x'");
        let att = log.finalize("sess-1");
        assert_eq!(att.entries.len(), 3);
        assert!(verify_attestation(&att));
    }

    #[test]
    fn attestation_detects_tampering() {
        let mut log = AttestationLog::new("sess-2");
        log.record("egress-allowed", "http://localhost:11434/v1");
        let mut att = log.finalize("sess-2");
        att.entries[1].detail = "http://localhost:11434/v1 -- tampered".to_string();
        assert!(!verify_attestation(&att));

        let mut log2 = AttestationLog::new("sess-3");
        log2.record("egress-allowed", "x");
        let mut att2 = log2.finalize("sess-3");
        att2.final_hash = "deadbeef".to_string();
        assert!(!verify_attestation(&att2));
    }

    #[test]
    fn attestation_rejects_wrong_scheme_or_empty() {
        let log = AttestationLog::new("sess-4");
        let mut att = log.finalize("sess-4");
        att.scheme = "something-else".to_string();
        assert!(!verify_attestation(&att));
        let empty = Attestation {
            scheme: ATTESTATION_SCHEME.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            session_id: "e".to_string(),
            finalized_at: "t".to_string(),
            entries: vec![],
            final_hash: String::new(),
        };
        assert!(!verify_attestation(&empty));
    }

    #[test]
    fn mode_blocks_remote_egress_and_counts() {
        let mut mode = SovereignMode::enable_with_probe("sess-5", OLLAMA_LOOPBACK_URL, false)
            .expect("enable without probe");
        assert!(mode.check_egress("http://localhost:11434/v1/chat").is_ok());
        let err = mode.check_egress("https://api.openai.com/v1").unwrap_err();
        assert!(matches!(err, SovereignError::BlockedUrl { .. }));
        assert_eq!(mode.allowed_egress_count(), 1);
        assert_eq!(mode.blocked_egress_count(), 1);

        assert!(mode
            .attestation_entries()
            .iter()
            .any(|e| e.kind == "egress-blocked"));
        let att = mode.disable();
        assert!(verify_attestation(&att));
        assert!(att.entries.iter().any(|e| e.kind == "mode-disabled"));
    }

    #[test]
    fn enable_rejects_remote_endpoint() {
        let err = SovereignMode::enable_with_probe("sess-6", "https://api.openai.com/v1", false)
            .unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
    }

    #[test]
    fn enable_probes_ollama_liveness() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("local addr").port();
        assert!(probe_ollama_running(port));
        drop(listener);

        assert!(!probe_ollama_running(1));
    }

    #[test]
    fn status_disabled_shape() {
        let s = SovereignStatus::disabled();
        assert!(!s.enabled);
        assert_eq!(s.ollama_endpoint, OLLAMA_LOOPBACK_URL);
    }

    #[test]
    fn error_messages_are_actionable() {
        let e = SovereignError::OllamaUnreachable {
            endpoint: "127.0.0.1:11434".to_string(),
        };
        assert!(e.to_string().contains("ollama serve"));
        let e2 = SovereignError::BlockedUrl {
            url: "https://x.example".to_string(),
            reason: "not loopback".to_string(),
        };
        assert!(e2.to_string().contains("Sovereign Mode blocked"));
    }
}
