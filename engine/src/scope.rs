use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::events::{DesktopEvent, DesktopEventSink};

pub const MAX_AUTO_FILES: usize = 20;

const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "rm -fr /",
    "sudo ",
    "mkfs",
    "dd if=",
    "> /dev/",
    "chmod -R /",
    ":(){",
];

#[derive(Clone, Debug, Default)]
pub struct ScopeDecl {
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalReason {
    WildcardPattern(String),
    PathEscape(String),
    OutsideRoot(String),
    TooManyFiles(usize),
}

impl ApprovalReason {
    pub fn message(&self) -> String {
        match self {
            ApprovalReason::WildcardPattern(p) => {
                format!("declaration contains wildcard pattern '{p}' - confirm the real file list")
            }
            ApprovalReason::PathEscape(p) => {
                format!("declaration contains path escape '{p}' - confirm this is intentional")
            }
            ApprovalReason::OutsideRoot(p) => {
                format!("declaration contains path outside the project root: '{p}'")
            }
            ApprovalReason::TooManyFiles(n) => {
                format!(
                    "declaration touches {n} files (limit {MAX_AUTO_FILES}) - confirm the scope"
                )
            }
        }
    }
}

pub fn needs_approval(decl: &ScopeDecl, root: &Path) -> Vec<ApprovalReason> {
    let mut reasons = Vec::new();
    let mut count = 0;
    for p in decl.files.iter().chain(decl.dirs.iter()) {
        count += 1;
        let s = p.to_string_lossy();
        if s.contains('*') || s.contains('?') {
            reasons.push(ApprovalReason::WildcardPattern(s.to_string()));
        }
        if has_escape(p) {
            reasons.push(ApprovalReason::PathEscape(s.to_string()));
        }
        if (p.is_absolute() || p.has_root()) && !p.starts_with(root) {
            reasons.push(ApprovalReason::OutsideRoot(s.to_string()));
        }
    }
    if count > MAX_AUTO_FILES {
        reasons.push(ApprovalReason::TooManyFiles(count));
    }
    reasons
}

fn has_escape(p: &Path) -> bool {
    p.components().any(|c| matches!(c, Component::ParentDir))
}

fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let joined = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        root.join(target)
    };
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if !norm.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            _ => norm.push(c.as_os_str()),
        }
    }
    if norm.starts_with(root) {
        Some(norm)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeViolation {
    pub tool: String,
    pub target: String,
    pub reason: String,
}

/// Stream-side scope guard.
///
/// The CLI runs the tools in its own process, so the Desktop can no longer
/// approve or deny a tool call before it runs. What it can still do is
/// observe the stream: every `assistant` tool call is checked against the
/// declared scope, and the first out-of-scope write / edit / destructive
/// shell command stops the child process (the caller stops on the returned
/// violation). Reads stay allowed, as before.
pub struct ScopeObserver {
    root: PathBuf,
    scope: Mutex<ScopeDecl>,
    sink: Arc<dyn DesktopEventSink>,
    violations: Mutex<Vec<ScopeViolation>>,
}

impl ScopeObserver {
    pub fn new(root: PathBuf, decl: ScopeDecl, sink: Arc<dyn DesktopEventSink>) -> Self {
        Self {
            root,
            scope: Mutex::new(decl),
            sink,
            violations: Mutex::new(Vec::new()),
        }
    }

    pub fn violations(&self) -> Vec<ScopeViolation> {
        self.violations.lock().unwrap().clone()
    }

    pub fn expand_scope(&self, files: &[PathBuf], dirs: &[PathBuf]) -> usize {
        let mut scope = self.scope.lock().unwrap();
        let mut added = 0;
        for f in files {
            if !scope.files.contains(f) {
                scope.files.push(f.clone());
                added += 1;
            }
        }
        for d in dirs {
            if !scope.dirs.contains(d) {
                scope.dirs.push(d.clone());
                added += 1;
            }
        }
        drop(scope);
        self.emit(
            "scope_expanded",
            &format!("scope expanded: {added} new path(s) approved by the user"),
        );
        added
    }

    pub fn in_scope(&self, target: &str) -> bool {
        let Some(abs) = resolve(&self.root, target) else {
            return false;
        };
        let scope = self.scope.lock().unwrap();
        if scope
            .files
            .iter()
            .any(|f| resolve(&self.root, &f.to_string_lossy()) == Some(abs.clone()))
        {
            return true;
        }
        scope.dirs.iter().any(|d| {
            let pat = d.to_string_lossy();

            let prefix = pat.strip_suffix("/*").unwrap_or(&pat);
            match resolve(&self.root, prefix) {
                Some(dir_abs) => abs.starts_with(&dir_abs),
                None => false,
            }
        })
    }

    /// Observe one event from the CLI subprocess stream. Returns the first
    /// scope violation found in an `assistant` tool call, if any.
    pub fn observe(&self, ev: &DesktopEvent) -> Option<ScopeViolation> {
        let DesktopEvent::AssistantMessage { tool_calls, .. } = ev else {
            return None;
        };
        for call in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
            let violation = match call.name.as_str() {
                "Read" | "Glob" | "Grep" => None,
                "Write" | "Edit" | "MultiEdit" => {
                    let target = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                    self.check_path(&call.name, target)
                }
                "Bash" => {
                    let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                    self.check_bash(cmd)
                }
                _ => None,
            };
            if violation.is_some() {
                return violation;
            }
        }
        None
    }

    fn deny(&self, tool: &str, target: &str, reason: &str) -> Option<ScopeViolation> {
        let v = ScopeViolation {
            tool: tool.to_string(),
            target: target.to_string(),
            reason: reason.to_string(),
        };
        self.violations.lock().unwrap().push(v.clone());
        self.emit(
            "scope_violation",
            &format!("agent tried to {tool} '{target}' - not in declared scope ({reason})"),
        );
        Some(v)
    }

    fn emit(&self, event: &str, text: &str) {
        self.sink.emit_desktop(DesktopEvent::SystemMessage {
            event: event.to_string(),
            text: text.to_string(),
        });
    }

    fn check_path(&self, tool: &str, target: &str) -> Option<ScopeViolation> {
        if target.is_empty() {
            return self.deny(tool, "(empty)", "no target path given");
        }
        if has_escape(Path::new(target)) && resolve(&self.root, target).is_none() {
            return self.deny(tool, target, "path escapes the project root");
        }
        if self.in_scope(target) {
            None
        } else {
            self.deny(tool, target, "declare it first with an expanded scope")
        }
    }

    fn check_bash(&self, cmd: &str) -> Option<ScopeViolation> {
        let lower = cmd.to_lowercase();
        for pat in DESTRUCTIVE_PATTERNS {
            if lower.contains(pat) {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("destructive pattern '{pat}' is never allowed"),
                );
            }
        }

        const MUTATING: &[&str] = &[
            "rm ", "mv ", "cp ", "mkdir ", "touch ", "tee ", "sed -i", ">", ">>", "chmod ",
            "chown ",
        ];
        if !MUTATING.iter().any(|m| lower.contains(m)) {
            return None;
        }
        for token in cmd.split_whitespace() {
            let t = token.trim_matches(|c| c == '"' || c == '\'' || c == ';' || c == '&');
            if t.is_empty() || t.starts_with('-') {
                continue;
            }
            let looks_like_path = t.contains('/')
                || t.contains('\\')
                || (t.contains('.') && !t.parse::<f64>().is_ok());
            if !looks_like_path {
                continue;
            }
            if has_escape(Path::new(t)) && resolve(&self.root, t).is_none() {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("path token '{t}' escapes the project root"),
                );
            }
            if !self.in_scope(t) {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("path token '{t}' is outside the declared scope"),
                );
            }
        }
        None
    }
}

pub fn emit_scope_declared(sink: &Arc<dyn DesktopEventSink>, decl: &ScopeDecl) {
    let files: Vec<String> = decl.files.iter().map(|p| p.display().to_string()).collect();
    let dirs: Vec<String> = decl.dirs.iter().map(|p| p.display().to_string()).collect();
    sink.emit_desktop(DesktopEvent::SystemMessage {
        event: "scope_declared".to_string(),
        text: format!(
            "agent declared scope: {} - files: [{}], dirs: [{}]",
            decl.description,
            files.join(", "),
            dirs.join(", ")
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{DesktopEventSink, ToolCallInfo};
    use serde_json::json;

    struct VecSink(std::sync::Mutex<Vec<DesktopEvent>>);

    impl DesktopEventSink for VecSink {
        fn emit_desktop(&self, ev: DesktopEvent) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn observer() -> (ScopeObserver, Arc<VecSink>) {
        let decl = ScopeDecl {
            files: vec![PathBuf::from("src/main.rs")],
            dirs: vec![PathBuf::from("docs")],
            description: "fix the typo".to_string(),
        };
        let sink = Arc::new(VecSink(std::sync::Mutex::new(Vec::new())));
        let obs = ScopeObserver::new(PathBuf::from("/proj"), decl, sink.clone());
        (obs, sink)
    }

    fn call_event(tool: &str, args: serde_json::Value) -> DesktopEvent {
        DesktopEvent::AssistantMessage {
            text: None,
            tool_calls: vec![ToolCallInfo {
                id: "c1".to_string(),
                name: tool.to_string(),
                arguments: args.to_string(),
            }],
            parent_tool_use_id: None,
        }
    }

    #[test]
    fn in_scope_write_passes() {
        let (obs, _) = observer();
        let ev = call_event("Write", json!({"file_path": "src/main.rs", "content": "x"}));
        assert_eq!(obs.observe(&ev), None);
        assert!(obs.violations().is_empty());
    }

    #[test]
    fn in_scope_dir_passes() {
        let (obs, _) = observer();
        let ev = call_event(
            "Write",
            json!({"file_path": "docs/guide.md", "content": "x"}),
        );
        assert_eq!(obs.observe(&ev), None);
    }

    #[test]
    fn out_of_scope_blocked_and_logged() {
        let (obs, sink) = observer();
        let ev = call_event(
            "Edit",
            json!({"file_path": "src/other.rs", "old_string": "a", "new_string": "b"}),
        );
        let v = obs
            .observe(&ev)
            .expect("out-of-scope edit must be a violation");
        assert_eq!(v.tool, "Edit");
        assert!(v.target.contains("other.rs"));
        assert_eq!(obs.violations().len(), 1);
        let events = sink.0.lock().unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            DesktopEvent::SystemMessage { event, .. } if event == "scope_violation"
        )));
    }

    #[test]
    fn path_escape_blocked() {
        let (obs, _) = observer();
        let ev = call_event(
            "Write",
            json!({"file_path": "../secret.txt", "content": "x"}),
        );
        assert!(obs.observe(&ev).is_some());
    }

    #[test]
    fn read_always_passes() {
        let (obs, _) = observer();
        let ev = call_event("Read", json!({"file_path": "/etc/passwd"}));
        assert_eq!(obs.observe(&ev), None);
    }

    #[test]
    fn destructive_bash_blocked() {
        let (obs, _) = observer();
        let ev = call_event("Bash", json!({"command": "rm -rf /tmp/cache"}));
        assert!(obs.observe(&ev).is_some());
        let ev2 = call_event("Bash", json!({"command": "rm -rf /"}));
        assert!(obs.observe(&ev2).is_some());
    }

    #[test]
    fn readonly_bash_passes() {
        let (obs, _) = observer();
        let ev = call_event("Bash", json!({"command": "cargo test"}));
        assert_eq!(obs.observe(&ev), None);
    }

    #[test]
    fn expansion_allows_new_file() {
        let (obs, _) = observer();
        assert_eq!(obs.expand_scope(&[PathBuf::from("src/new.rs")], &[]), 1);
        let ev = call_event("Write", json!({"file_path": "src/new.rs", "content": "x"}));
        assert_eq!(obs.observe(&ev), None);
    }

    #[test]
    fn needs_approval_flags_risky_decls() {
        let root = Path::new("/proj");
        let wild = ScopeDecl {
            files: vec![PathBuf::from("src/*.rs")],
            dirs: vec![],
            description: "x".into(),
        };
        let r = needs_approval(&wild, root);
        assert!(r
            .iter()
            .any(|x| matches!(x, ApprovalReason::WildcardPattern(_))));

        let esc = ScopeDecl {
            files: vec![PathBuf::from("../x")],
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&esc, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::PathEscape(_))));

        let outside = ScopeDecl {
            files: vec![PathBuf::from("/etc/hosts")],
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&outside, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::OutsideRoot(_))));

        let many = ScopeDecl {
            files: (0..25).map(|i| PathBuf::from(format!("f{i}.rs"))).collect(),
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&many, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::TooManyFiles(25))));

        let clean = ScopeDecl {
            files: vec![PathBuf::from("a.rs")],
            dirs: vec![PathBuf::from("docs")],
            description: "x".into(),
        };
        assert!(needs_approval(&clean, root).is_empty());
    }
}
