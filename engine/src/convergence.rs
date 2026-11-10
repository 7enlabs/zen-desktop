use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::events::{DesktopEvent, DesktopEventSink};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopPattern {
    SameFileFailingEdits { path: String, attempts: u32 },

    RepeatingError { signature: String, attempts: u32 },

    StagnantRounds { rounds: u32 },
}

impl LoopPattern {
    pub fn describe(&self) -> String {
        match self {
            LoopPattern::SameFileFailingEdits { path, attempts } => format!(
                "edited '{path}' {attempts} times in a row, and verification failed after every edit"
            ),
            LoopPattern::RepeatingError { signature, attempts } => format!(
                "the same error kept repeating {attempts} times in a row: '{signature}'"
            ),
            LoopPattern::StagnantRounds { rounds } => {
                format!("{rounds} tool rounds in a row changed no files (no progress)")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConvergencePolicy {
    pub max_same_file_edits: u32,

    pub max_same_error: u32,

    pub max_stagnant_rounds: u32,
}

impl Default for ConvergencePolicy {
    fn default() -> Self {
        Self {
            max_same_file_edits: 3,
            max_same_error: 3,
            max_stagnant_rounds: 5,
        }
    }
}

impl ConvergencePolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_same_file_edits == 0 {
            anyhow::bail!("max_same_file_edits must be >= 1");
        }
        if self.max_same_error == 0 {
            anyhow::bail!("max_same_error must be >= 1");
        }
        if self.max_stagnant_rounds == 0 {
            anyhow::bail!("max_stagnant_rounds must be >= 1");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct AttemptRecord {
    path: String,
    verify_passed: bool,
    error_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopDetectedPayload {
    pub pattern: LoopPattern,
    pub attempts: u32,
    pub files: Vec<String>,

    pub restore_point: Option<String>,
}

pub const LOOP_DETECTED_EVENT: &str = "loop_detected";

pub const ERROR_PREVIEW_LIMIT: usize = 500;

pub struct LoopDetector {
    policy: ConvergencePolicy,

    failing_edits: HashMap<String, u32>,

    error_counts: HashMap<String, u32>,

    stagnant_rounds: u32,

    history: Vec<AttemptRecord>,

    pending_paths: Vec<String>,

    triggered: bool,
}

impl LoopDetector {
    pub fn new(policy: ConvergencePolicy) -> anyhow::Result<Self> {
        policy.validate()?;
        Ok(Self {
            policy,
            failing_edits: HashMap::new(),
            error_counts: HashMap::new(),
            stagnant_rounds: 0,
            history: Vec::new(),
            pending_paths: Vec::new(),
            triggered: false,
        })
    }

    pub fn record_edit(&mut self, path: &str) {
        let path = normalize_path(path);
        *self.failing_edits.entry(path.clone()).or_insert(0) += 1;
        if !self.pending_paths.contains(&path) {
            self.pending_paths.push(path);
        }

        self.stagnant_rounds = 0;
    }

    pub fn record_verify(&mut self, passed: bool, error: Option<&str>) {
        if passed {
            for path in self.pending_paths.drain(..) {
                self.failing_edits.remove(&path);
            }
            self.error_counts.clear();
            self.history.push(AttemptRecord {
                path: String::new(),
                verify_passed: true,
                error_preview: None,
            });
            return;
        }
        let error_preview = error.map(|e| truncate(e, ERROR_PREVIEW_LIMIT));
        for path in self.pending_paths.drain(..) {
            self.history.push(AttemptRecord {
                path,
                verify_passed: false,
                error_preview: error_preview.clone(),
            });
        }
        if let Some(raw) = error {
            let sig = normalize_error(raw);

            self.error_counts.retain(|k, _| *k == sig);
            *self.error_counts.entry(sig).or_insert(0) += 1;
        }
    }

    pub fn record_stagnant_round(&mut self) {
        self.stagnant_rounds += 1;
    }

    pub fn record_progress_round(&mut self) {
        self.stagnant_rounds = 0;
    }

    pub fn check(&mut self) -> Option<LoopPattern> {
        if self.triggered {
            return None;
        }

        if let Some((path, &count)) = self
            .failing_edits
            .iter()
            .find(|(_, &c)| c >= self.policy.max_same_file_edits)
        {
            self.triggered = true;
            return Some(LoopPattern::SameFileFailingEdits {
                path: path.clone(),
                attempts: count,
            });
        }

        if let Some((sig, &count)) = self
            .error_counts
            .iter()
            .find(|(_, &c)| c >= self.policy.max_same_error)
        {
            self.triggered = true;
            return Some(LoopPattern::RepeatingError {
                signature: sig.clone(),
                attempts: count,
            });
        }

        if self.stagnant_rounds >= self.policy.max_stagnant_rounds {
            self.triggered = true;
            return Some(LoopPattern::StagnantRounds {
                rounds: self.stagnant_rounds,
            });
        }
        None
    }

    pub fn reset(&mut self) {
        self.failing_edits.clear();
        self.error_counts.clear();
        self.stagnant_rounds = 0;
        self.history.clear();
        self.pending_paths.clear();
        self.triggered = false;
    }

    pub fn replan_prompt(&self, pattern: &LoopPattern, restore_point: Option<&str>) -> String {
        let mut out = String::from(
            "CONVERGENCE GUARD: I stopped your current approach because it is looping.\n\n",
        );
        out.push_str(&format!("Pattern detected: {}.\n\n", pattern.describe()));
        out.push_str("What was tried:\n");
        let recent: Vec<&AttemptRecord> = self.history.iter().rev().take(6).collect();
        if recent.is_empty() {
            out.push_str("  - (no edit history recorded)\n");
        }
        for rec in recent.into_iter().rev() {
            if rec.verify_passed {
                out.push_str("  - a verify step passed (approach worked at that point)\n");
            } else {
                out.push_str(&format!("  - edit '{}'", rec.path));
                if let Some(err) = &rec.error_preview {
                    out.push_str(&format!(" -> verify FAILED: {}", first_line(err)));
                } else {
                    out.push_str(" -> verify FAILED");
                }
                out.push('\n');
            }
        }
        if let Some(id) = restore_point {
            out.push_str(&format!(
                "\nA checkpoint '{id}' was saved before the loop started; you can restore \
                 the workspace to it instead of untangling the failed edits.\n"
            ));
        }
        out.push_str(
            "\nYou must now propose a DIFFERENT approach. Do NOT retry the same edit, \
             do NOT re-run the same failing command. First explain, in 2-3 sentences, \
             what you think the real root cause is and what you will do differently, \
             then act.",
        );
        out
    }

    pub fn emit_loop_detected(
        &self,
        pattern: &LoopPattern,
        restore_point: Option<&str>,
        sink: &dyn DesktopEventSink,
    ) -> anyhow::Result<()> {
        let attempts = match pattern {
            LoopPattern::SameFileFailingEdits { attempts, .. } => *attempts,
            LoopPattern::RepeatingError { attempts, .. } => *attempts,
            LoopPattern::StagnantRounds { rounds } => *rounds,
        };
        let mut files: Vec<String> = self.failing_edits.keys().cloned().collect();
        files.sort();
        let payload = LoopDetectedPayload {
            pattern: pattern.clone(),
            attempts,
            files,
            restore_point: restore_point.map(str::to_string),
        };
        let text = serde_json::to_string(&payload)?;
        sink.emit_desktop(DesktopEvent::SystemMessage {
            event: LOOP_DETECTED_EVENT.to_string(),
            text,
        });
        Ok(())
    }
}

fn normalize_path(p: &str) -> String {
    p.trim().replace('\\', "/")
}

fn normalize_error(raw: &str) -> String {
    let first = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut sig = String::with_capacity(first.len());
    let mut in_digits = false;
    for ch in first.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                sig.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            sig.push(ch.to_ascii_lowercase());
        }
    }
    truncate(&sig, 160)
}

fn truncate(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        s.to_string()
    } else {
        let mut end = limit;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

pub const CONVERGENCE_STOP_EVENT: &str = "convergence_stop";

struct MonitorState {
    detector: LoopDetector,

    calls: HashMap<String, (String, Option<String>)>,
    round_had_tools: bool,
    round_had_edits: bool,

    round_had_progress: bool,

    seen_outputs: std::collections::HashSet<u64>,

    fired: u32,

    stopped: Option<String>,
}

pub struct ConvergenceMonitor {
    state: std::sync::Mutex<MonitorState>,
    sink: std::sync::Arc<dyn DesktopEventSink>,
}

impl ConvergenceMonitor {
    pub fn new(
        policy: ConvergencePolicy,
        sink: std::sync::Arc<dyn DesktopEventSink>,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        Ok(std::sync::Arc::new(Self {
            state: std::sync::Mutex::new(MonitorState {
                detector: LoopDetector::new(policy)?,
                calls: HashMap::new(),
                round_had_tools: false,
                round_had_edits: false,
                round_had_progress: false,
                seen_outputs: std::collections::HashSet::new(),
                fired: 0,
                stopped: None,
            }),
            sink,
        }))
    }

    pub fn times_fired(&self) -> u32 {
        self.state.lock().unwrap().fired
    }

    pub fn stop_reason(&self) -> Option<String> {
        self.state.lock().unwrap().stopped.clone()
    }

    /// Observe one event from the CLI subprocess stream. Returns the stop
    /// reason when a loop is detected; the caller stops the child process.
    ///
    /// Round boundaries: each `assistant` message is one model call, so a
    /// new assistant message ends the previous round, and `SessionEnd`
    /// ends the last one. The old in-process monitor could inject a
    /// re-plan prompt into the agent loop and let it continue; across the
    /// process boundary that is impossible, so the first detected loop
    /// stops the run and the pattern is reported instead.
    pub fn observe(&self, ev: &DesktopEvent) -> Option<String> {
        match ev {
            DesktopEvent::AssistantMessage { tool_calls, .. } => {
                // A new model call starts: close the previous round first.
                self.end_round();
                let mut st = self.state.lock().unwrap();
                for call in tool_calls {
                    let path = serde_json::from_str::<serde_json::Value>(&call.arguments)
                        .ok()
                        .and_then(|v| {
                            v.get("file_path")
                                .or_else(|| v.get("path"))
                                .and_then(|p| p.as_str())
                                .map(str::to_string)
                        });
                    st.calls.insert(call.id.clone(), (call.name.clone(), path));
                    st.round_had_tools = true;
                }
                drop(st);
                self.stop_reason()
            }
            DesktopEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => {
                self.observe_tool_result(tool_use_id, content, *is_error);
                self.stop_reason()
            }
            DesktopEvent::SessionEnd { .. } => {
                self.end_round();
                self.stop_reason()
            }
            _ => self.stop_reason(),
        }
    }

    fn observe_tool_result(&self, tool_use_id: &str, content: &str, is_error: bool) {
        let mut st = self.state.lock().unwrap();
        let Some((name, path)) = st.calls.remove(tool_use_id) else {
            return;
        };

        if content.starts_with("CONVERGENCE GUARD:")
            || content.starts_with("Task stopped by the convergence guard")
        {
            return;
        }
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (name.as_str(), is_error, content).hash(&mut h);
            h.finish()
        };
        if st.seen_outputs.insert(key) {
            st.round_had_progress = true;
        }
        match (name.as_str(), path) {
            ("Write" | "Edit" | "MultiEdit", Some(p)) if !is_error => {
                st.detector.record_edit(&p);
                st.round_had_edits = true;
            }
            ("Bash", _) => {
                if is_error {
                    st.detector.record_verify(false, Some(content));
                } else {
                    st.detector.record_verify(true, None);
                    st.round_had_progress = true;
                }
            }
            _ => {}
        }
        self.check_locked(&mut st);
    }

    fn end_round(&self) {
        let mut st = self.state.lock().unwrap();
        if st.round_had_tools {
            if st.round_had_edits || st.round_had_progress {
                st.detector.record_progress_round();
            } else {
                st.detector.record_stagnant_round();
            }
        }
        st.round_had_tools = false;
        st.round_had_edits = false;
        st.round_had_progress = false;
        self.check_locked(&mut st);
    }

    fn check_locked(&self, st: &mut MonitorState) {
        if st.stopped.is_some() {
            return;
        }
        let Some(pattern) = st.detector.check() else {
            return;
        };
        st.fired += 1;
        if let Err(e) = st.detector.emit_loop_detected(&pattern, None, &*self.sink) {
            eprintln!("convergence guard: could not emit loop_detected: {e}");
        }
        let reason = format!(
            "Task stopped by the convergence guard: {}",
            pattern.describe()
        );
        self.sink.emit_desktop(DesktopEvent::SystemMessage {
            event: CONVERGENCE_STOP_EVENT.to_string(),
            text: reason.clone(),
        });
        st.stopped = Some(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct VecSink(std::sync::Mutex<Vec<DesktopEvent>>);

    impl DesktopEventSink for VecSink {
        fn emit_desktop(&self, ev: DesktopEvent) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn detector() -> LoopDetector {
        LoopDetector::new(ConvergencePolicy::default()).unwrap()
    }

    #[test]
    fn detects_repeated_failing_edits_on_same_file() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("test foo FAILED"));
        }
        match d.check() {
            Some(LoopPattern::SameFileFailingEdits { path, attempts }) => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(attempts, 3);
            }
            other => panic!("expected SameFileFailingEdits, got {other:?}"),
        }
    }

    #[test]
    fn detects_repeating_error_signature() {
        let mut d = detector();

        let files = ["a.rs", "b.rs", "c.rs"];
        for f in files {
            d.record_edit(f);
            d.record_verify(
                false,
                Some("error[E0308]: mismatched types\n  --> src/lib.rs:42"),
            );
        }
        match d.check() {
            Some(LoopPattern::RepeatingError {
                signature,
                attempts,
            }) => {
                assert_eq!(attempts, 3);
                assert!(signature.contains("mismatched types"), "sig: {signature}");
            }
            other => panic!("expected RepeatingError, got {other:?}"),
        }
    }

    #[test]
    fn no_false_positive_on_two_attempts() {
        let mut d = detector();
        for _ in 0..2 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert_eq!(d.check(), None);

        d.record_stagnant_round();
        d.record_stagnant_round();
        assert_eq!(d.check(), None);
    }

    #[test]
    fn passing_verify_resets_counters() {
        let mut d = detector();
        for _ in 0..2 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        d.record_edit("src/main.rs");
        d.record_verify(true, None);
        assert_eq!(d.check(), None);
    }

    #[test]
    fn progress_round_resets_stagnation() {
        let mut d = detector();
        for _ in 0..4 {
            d.record_stagnant_round();
        }
        d.record_progress_round();
        for _ in 0..4 {
            d.record_stagnant_round();
        }
        assert_eq!(d.check(), None);
        d.record_stagnant_round();
        assert!(matches!(
            d.check(),
            Some(LoopPattern::StagnantRounds { rounds: 5 })
        ));
    }

    #[test]
    fn stagnant_rounds_trigger_and_edit_resets() {
        let mut d = detector();
        for _ in 0..4 {
            d.record_stagnant_round();
        }
        assert_eq!(d.check(), None);

        d.record_edit("src/main.rs");
        for _ in 0..5 {
            d.record_stagnant_round();
        }
        match d.check() {
            Some(LoopPattern::StagnantRounds { rounds }) => assert_eq!(rounds, 5),
            other => panic!("expected StagnantRounds, got {other:?}"),
        }
    }

    #[test]
    fn check_fires_only_once_until_reset() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert!(d.check().is_some());
        assert_eq!(d.check(), None);
        d.reset();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert!(d.check().is_some());
    }

    #[test]
    fn replan_prompt_contains_attempt_history() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/broken.rs");
            d.record_verify(false, Some("error[E0308]: mismatched types"));
        }
        let pattern = d.check().expect("loop should fire");
        let prompt = d.replan_prompt(&pattern, Some("abc1234"));
        assert!(prompt.contains("src/broken.rs"), "prompt: {prompt}");
        assert!(prompt.contains("mismatched types"), "prompt: {prompt}");
        assert!(prompt.contains("abc1234"), "prompt: {prompt}");
        assert!(prompt.contains("DIFFERENT"), "prompt: {prompt}");
    }

    #[test]
    fn replan_prompt_without_restore_point() {
        let mut d = detector();
        for _ in 0..5 {
            d.record_stagnant_round();
        }
        let pattern = d.check().expect("loop should fire");
        let prompt = d.replan_prompt(&pattern, None);
        assert!(!prompt.contains("checkpoint"), "prompt: {prompt}");
        assert!(prompt.contains("no progress"), "prompt: {prompt}");
    }

    #[test]
    fn emit_loop_detected_writes_system_event() {
        struct VecSink(std::sync::Mutex<Vec<DesktopEvent>>);
        impl DesktopEventSink for VecSink {
            fn emit_desktop(&self, ev: DesktopEvent) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let sink = VecSink(std::sync::Mutex::new(Vec::new()));
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        let pattern = d.check().expect("loop should fire");
        d.emit_loop_detected(&pattern, Some("abc1234"), &sink)
            .unwrap();
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DesktopEvent::SystemMessage { event, text } => {
                assert_eq!(event, LOOP_DETECTED_EVENT);
                let payload: LoopDetectedPayload = serde_json::from_str(text).unwrap();
                assert_eq!(payload.attempts, 3);
                assert_eq!(payload.restore_point.as_deref(), Some("abc1234"));
                assert_eq!(payload.files, vec!["src/main.rs".to_string()]);
                assert!(matches!(
                    payload.pattern,
                    LoopPattern::SameFileFailingEdits { .. }
                ));
            }
            other => panic!("expected SystemMessage, got {other:?}"),
        }
    }

    #[test]
    fn error_normalization_folds_numbers() {
        let mut d = detector();
        let files = ["a.rs", "b.rs", "c.rs"];
        for (i, f) in files.iter().enumerate() {
            d.record_edit(f);
            d.record_verify(
                false,
                Some(&format!(
                    "error[E0308]: mismatched types\n  --> src/lib.rs:{}",
                    40 + i
                )),
            );
        }
        match d.check() {
            Some(LoopPattern::RepeatingError { attempts, .. }) => assert_eq!(attempts, 3),
            other => panic!("expected RepeatingError, got {other:?}"),
        }
    }

    #[test]
    fn policy_validation_rejects_zeros() {
        let bad = ConvergencePolicy {
            max_same_file_edits: 0,
            ..ConvergencePolicy::default()
        };
        assert!(LoopDetector::new(bad).is_err());
    }

    fn vec_sink() -> std::sync::Arc<VecSink> {
        std::sync::Arc::new(VecSink(std::sync::Mutex::new(Vec::new())))
    }

    fn edit_call(id: &str) -> DesktopEvent {
        DesktopEvent::AssistantMessage {
            text: None,
            tool_calls: vec![crate::events::ToolCallInfo {
                id: id.to_string(),
                name: "Edit".to_string(),
                arguments: r#"{"file_path": "src/main.rs", "old_string": "a", "new_string": "b"}"#
                    .to_string(),
            }],
            parent_tool_use_id: None,
        }
    }

    fn bash_result(id: &str, content: &str, is_error: bool) -> DesktopEvent {
        DesktopEvent::ToolResult {
            tool_use_id: id.to_string(),
            content: content.to_string(),
            is_error,
            parent_tool_use_id: None,
        }
    }

    fn bash_call(id: &str) -> DesktopEvent {
        DesktopEvent::AssistantMessage {
            text: None,
            tool_calls: vec![crate::events::ToolCallInfo {
                id: id.to_string(),
                name: "Bash".to_string(),
                arguments: r#"{"command": "cargo test"}"#.to_string(),
            }],
            parent_tool_use_id: None,
        }
    }

    #[test]
    fn monitor_stops_on_repeated_failing_edits_from_stream() {
        let sink = vec_sink();
        let monitor = ConvergenceMonitor::new(ConvergencePolicy::default(), sink.clone()).unwrap();
        for i in 0..3 {
            let eid = format!("e{i}");
            let bid = format!("b{i}");
            assert_eq!(monitor.observe(&edit_call(&eid)), None);
            // The third edit of the same file is the one that trips the
            // guard: the stop reason comes back from the edit's own result.
            let stopped = monitor.observe(&bash_result(&eid, "edit ok", false));
            if i < 2 {
                assert_eq!(stopped, None, "round {i} must not stop yet");
            } else {
                let reason = stopped.expect("third failing edit must stop the run");
                assert!(reason.contains("src/main.rs"), "{reason}");
                assert!(reason.contains("convergence guard"), "{reason}");
            }
            if i < 2 {
                assert_eq!(monitor.observe(&bash_call(&bid)), None);
                assert_eq!(
                    monitor.observe(&bash_result(&bid, "error: test failed", true)),
                    None,
                    "round {i} verify must not stop yet"
                );
            }
        }
        assert_eq!(monitor.times_fired(), 1);
    }

    #[test]
    fn monitor_does_not_stop_on_passing_verify() {
        let sink = vec_sink();
        let monitor = ConvergenceMonitor::new(ConvergencePolicy::default(), sink.clone()).unwrap();
        for i in 0..2 {
            let eid = format!("e{i}");
            let bid = format!("b{i}");
            assert_eq!(monitor.observe(&edit_call(&eid)), None);
            assert_eq!(monitor.observe(&bash_result(&eid, "edit ok", false)), None);
            assert_eq!(monitor.observe(&bash_call(&bid)), None);
            // A passing verify clears the failing-edit count for the file.
            assert_eq!(monitor.observe(&bash_result(&bid, "ok", false)), None);
        }
        assert_eq!(monitor.stop_reason(), None);
        assert_eq!(monitor.times_fired(), 0);
    }
}
