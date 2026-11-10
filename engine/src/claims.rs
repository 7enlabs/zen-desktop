use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::events::DesktopEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    TestRun,

    FileWritten,

    CommandExecuted,

    BuildPassed,

    ExternalFact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,

    pub detail: String,
    pub verified: bool,
}

impl Evidence {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>, verified: bool) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            verified,
        }
    }

    pub fn verify_report(passed: bool, exit_code: Option<i32>, output_preview: &str) -> Self {
        let detail = match exit_code {
            Some(code) => format!("verify exit_code={code} output: {output_preview}"),
            None => format!("verify output: {output_preview}"),
        };
        Self::new("verify_report", detail, passed)
    }

    pub fn tool_result(detail: impl Into<String>, is_error: bool) -> Self {
        Self::new("tool_result", detail, !is_error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Unverified,

    Verified,

    Contradicted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: u64,
    pub kind: ClaimKind,
    pub statement: String,
    pub evidence: Vec<Evidence>,
    pub status: ClaimStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimCounts {
    pub verified: u64,
    pub unverified: u64,
    pub contradicted: u64,
}

#[derive(Debug, Default)]
struct RegistryInner {
    claims: Vec<Claim>,
    next_id: u64,
}

#[derive(Debug, Default)]
pub struct ClaimRegistry {
    inner: Mutex<RegistryInner>,
}

impl ClaimRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_claim(&self, kind: ClaimKind, statement: impl Into<String>) -> u64 {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        g.claims.push(Claim {
            id,
            kind,
            statement: statement.into(),
            evidence: Vec::new(),
            status: ClaimStatus::Unverified,
        });
        id
    }

    pub fn attach_evidence(&self, id: u64, evidence: Evidence) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(claim) = g.claims.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        claim.evidence.push(evidence);
        claim.status = evaluate(&claim.evidence);
        true
    }

    pub fn verify(&self, id: u64) -> Option<ClaimStatus> {
        let mut g = self.inner.lock().unwrap();
        let claim = g.claims.iter_mut().find(|c| c.id == id)?;
        claim.status = evaluate(&claim.evidence);
        Some(claim.status)
    }

    pub fn attach_verify_evidence(
        &self,
        id: u64,
        passed: bool,
        exit_code: Option<i32>,
        output_preview: &str,
    ) -> bool {
        self.attach_evidence(
            id,
            Evidence::verify_report(passed, exit_code, output_preview),
        )
    }

    pub fn counts(&self) -> ClaimCounts {
        let g = self.inner.lock().unwrap();
        let mut c = ClaimCounts::default();
        for claim in &g.claims {
            match claim.status {
                ClaimStatus::Verified => c.verified += 1,
                ClaimStatus::Unverified => c.unverified += 1,
                ClaimStatus::Contradicted => c.contradicted += 1,
            }
        }
        c
    }

    pub fn claims(&self) -> Vec<Claim> {
        self.inner.lock().unwrap().claims.clone()
    }

    pub fn get(&self, id: u64) -> Option<Claim> {
        self.inner
            .lock()
            .unwrap()
            .claims
            .iter()
            .find(|c| c.id == id)
            .cloned()
    }
}

fn evaluate(evidence: &[Evidence]) -> ClaimStatus {
    if evidence.iter().any(|e| !e.verified) {
        ClaimStatus::Contradicted
    } else if evidence.is_empty() {
        ClaimStatus::Unverified
    } else {
        ClaimStatus::Verified
    }
}

pub fn detect_claims(text: &str) -> Vec<(ClaimKind, String)> {
    const PATTERNS: &[(ClaimKind, &[&str])] = &[
        (
            ClaimKind::TestRun,
            &[
                "tests pass",
                "test suite passes",
                "all tests pass",
                "tests are passing",
                "tests now pass",
            ],
        ),
        (
            ClaimKind::BuildPassed,
            &[
                "build succeeds",
                "build passes",
                "build is green",
                "compiles successfully",
                "compilation succeeds",
            ],
        ),
        (
            ClaimKind::FileWritten,
            &[
                "file written",
                "wrote the file",
                "file created",
                "changes written",
                "file has been written",
            ],
        ),
        (
            ClaimKind::CommandExecuted,
            &[
                "command succeeded",
                "command ran successfully",
                "executed successfully",
                "command completed successfully",
            ],
        ),
    ];
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for (kind, patterns) in PATTERNS {
        if let Some(hit) = patterns.iter().find(|p| lower.contains(*p)) {
            let statement = sentence_around(&lower, hit).unwrap_or_else(|| (*hit).to_string());
            out.push((*kind, statement));
        }
    }
    out
}

fn sentence_around(text: &str, needle: &str) -> Option<String> {
    let pos = text.find(needle)?;
    let start = text[..pos].rfind(['.', '\n']).map_or(0, |i| i + 1);
    let end = text[pos..]
        .find(['.', '\n'])
        .map_or(text.len(), |i| pos + i + 1);
    let s: String = text[start..end].trim().chars().take(160).collect();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn looks_like_test_command(arguments: &str) -> bool {
    let a = arguments.to_lowercase();
    a.contains("cargo test")
        || a.contains("pytest")
        || a.contains("npm test")
        || a.contains("go test")
}

fn looks_like_build_command(arguments: &str) -> bool {
    let a = arguments.to_lowercase();
    a.contains("cargo build") || a.contains("npm run build") || a.contains("go build")
}

fn outcome_from_output(content: &str) -> Option<bool> {
    let lower = content.to_lowercase();
    if lower.contains("test result: failed") || lower.contains("panicked") {
        return Some(false);
    }
    if failed_count(&lower).is_some_and(|n| n > 0) {
        return Some(false);
    }
    if (lower.contains("error:") || lower.contains("error[")) && !lower.contains("0 error") {
        return Some(false);
    }
    const PASS: &[&str] = &[
        "test result: ok",
        "0 failed",
        "passed",
        "build successful",
        "finished",
    ];
    if PASS.iter().any(|m| lower.contains(m)) {
        return Some(true);
    }
    None
}

fn failed_count(lower: &str) -> Option<u64> {
    let mut max: Option<u64> = None;
    let mut search: &str = lower;
    while let Some(pos) = search.find("failed") {
        let before = search[..pos].trim_end();
        let digits: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if let Ok(n) = digits.parse::<u64>() {
            max = Some(max.map_or(n, |m| m.max(n)));
        }
        search = &search[pos + "failed".len()..];
    }
    max
}

/// Observes the CLI subprocess stream and maintains the claim registry:
/// claims are detected in assistant text, and tool results attach as
/// evidence for them. The observer does not forward events; the run loop
/// owns the sink and calls [`ClaimObserver::observe`] for each event before
/// emitting it, so the final `SessionEnd` can carry the registry counts.
pub struct ClaimObserver {
    registry: Arc<ClaimRegistry>,

    calls: Mutex<HashMap<String, (String, String)>>,
}

impl ClaimObserver {
    pub fn new(registry: Arc<ClaimRegistry>) -> Self {
        Self {
            registry,
            calls: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry(&self) -> &Arc<ClaimRegistry> {
        &self.registry
    }

    pub fn observe(&self, ev: &DesktopEvent) {
        match ev {
            DesktopEvent::AssistantMessage {
                text, tool_calls, ..
            } => {
                for tc in tool_calls {
                    self.calls
                        .lock()
                        .unwrap()
                        .insert(tc.id.clone(), (tc.name.clone(), tc.arguments.clone()));
                }
                if let Some(text) = text.as_deref().filter(|t| !t.is_empty()) {
                    for (kind, statement) in detect_claims(text) {
                        self.registry.register_claim(kind, statement);
                    }
                }
            }
            DesktopEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => {
                let call = self.calls.lock().unwrap().remove(tool_use_id);
                if let Some((name, arguments)) = call {
                    let kind = if name == "Bash" && looks_like_test_command(&arguments) {
                        Some(ClaimKind::TestRun)
                    } else if name == "Bash" && looks_like_build_command(&arguments) {
                        Some(ClaimKind::BuildPassed)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        let supported = outcome_from_output(content).unwrap_or(!is_error);
                        let detail: String = content
                            .chars()
                            .take(300)
                            .collect::<String>()
                            .trim()
                            .to_string();

                        let target = self
                            .registry
                            .claims()
                            .into_iter()
                            .rev()
                            .find(|c| c.kind == kind && c.status == ClaimStatus::Unverified)
                            .map(|c| c.id);
                        if let Some(id) = target {
                            self.registry.attach_evidence(
                                id,
                                Evidence::tool_result(
                                    format!("{name} result: {detail}"),
                                    !supported,
                                ),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ToolCallInfo;

    fn assistant(text: &str, calls: Vec<ToolCallInfo>) -> DesktopEvent {
        DesktopEvent::AssistantMessage {
            text: Some(text.to_string()),
            tool_calls: calls,
            parent_tool_use_id: None,
        }
    }

    fn tool_result(id: &str, content: &str, is_error: bool) -> DesktopEvent {
        DesktopEvent::ToolResult {
            tool_use_id: id.to_string(),
            content: content.to_string(),
            is_error,
            parent_tool_use_id: None,
        }
    }
    #[test]
    fn lifecycle_register_unverified_then_verified() {
        let r = ClaimRegistry::new();
        let id = r.register_claim(ClaimKind::TestRun, "tests pass");
        assert_eq!(r.get(id).unwrap().status, ClaimStatus::Unverified);
        assert!(r.attach_evidence(id, Evidence::new("verify_report", "12 passed", true)));
        assert_eq!(r.get(id).unwrap().status, ClaimStatus::Verified);
    }

    #[test]
    fn never_auto_verified_without_evidence() {
        let r = ClaimRegistry::new();
        let id = r.register_claim(ClaimKind::BuildPassed, "build succeeds");

        assert_eq!(r.verify(id), Some(ClaimStatus::Unverified));
        assert_eq!(r.counts().unverified, 1);
        assert_eq!(r.counts().verified, 0);
    }

    #[test]
    fn refuting_evidence_contradicts() {
        let r = ClaimRegistry::new();
        let id = r.register_claim(ClaimKind::TestRun, "tests pass");
        assert!(r.attach_evidence(id, Evidence::verify_report(false, Some(1), "2 failed")));
        assert_eq!(r.get(id).unwrap().status, ClaimStatus::Contradicted);

        assert!(r.attach_evidence(id, Evidence::new("manual", "reran ok", true)));
        assert_eq!(r.get(id).unwrap().status, ClaimStatus::Contradicted);
    }

    #[test]
    fn unknown_id_is_rejected() {
        let r = ClaimRegistry::new();
        assert!(!r.attach_evidence(99, Evidence::new("x", "y", true)));
        assert_eq!(r.verify(99), None);
    }

    #[test]
    fn session_end_counts_are_correct() {
        let r = ClaimRegistry::new();
        let a = r.register_claim(ClaimKind::TestRun, "tests pass");
        let b = r.register_claim(ClaimKind::BuildPassed, "build succeeds");
        let _c = r.register_claim(ClaimKind::FileWritten, "file written");
        r.attach_evidence(a, Evidence::new("verify_report", "ok", true));
        r.attach_evidence(b, Evidence::verify_report(false, Some(101), "error"));
        let counts = r.counts();
        assert_eq!(
            counts,
            ClaimCounts {
                verified: 1,
                unverified: 1,
                contradicted: 1
            }
        );
    }

    #[test]
    fn detect_claims_finds_explicit_assertions() {
        let found = detect_claims("Done. All tests pass and the build succeeds.");
        let kinds: Vec<ClaimKind> = found.iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&ClaimKind::TestRun));
        assert!(kinds.contains(&ClaimKind::BuildPassed));
    }

    #[test]
    fn detect_claims_ignores_plans() {
        assert!(detect_claims("I will run the tests next.").is_empty());
        assert!(detect_claims("").is_empty());
    }

    #[test]
    fn claiming_observer_registers_claims_from_assistant_text() {
        let registry = Arc::new(ClaimRegistry::new());
        let observer = ClaimObserver::new(registry.clone());
        observer.observe(&assistant("All tests pass now.", vec![]));
        let claims = registry.claims();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].kind, ClaimKind::TestRun);
        assert_eq!(claims[0].status, ClaimStatus::Unverified);
    }

    #[test]
    fn claiming_observer_attaches_tool_evidence_to_matching_claim() {
        let registry = Arc::new(ClaimRegistry::new());
        let observer = ClaimObserver::new(registry.clone());

        observer.observe(&assistant(
            "All tests pass.",
            vec![ToolCallInfo {
                id: "t1".to_string(),
                name: "Bash".to_string(),
                arguments: r#"{"command": "cargo test"}"#.to_string(),
            }],
        ));

        observer.observe(&tool_result(
            "t1",
            "test result: ok. 12 passed; 0 failed",
            false,
        ));
        let claims = registry.claims();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].status, ClaimStatus::Verified);
        assert_eq!(claims[0].evidence.len(), 1);
    }

    #[test]
    fn claiming_observer_contradicts_on_failing_tool_result() {
        let registry = Arc::new(ClaimRegistry::new());
        let observer = ClaimObserver::new(registry.clone());
        observer.observe(&assistant(
            "The build succeeds.",
            vec![ToolCallInfo {
                id: "t2".to_string(),
                name: "Bash".to_string(),
                arguments: r#"{"command": "cargo build"}"#.to_string(),
            }],
        ));
        observer.observe(&tool_result("t2", "error: could not compile", true));
        let claims = registry.claims();
        assert_eq!(claims[0].status, ClaimStatus::Contradicted);
    }
}
