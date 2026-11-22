use std::collections::HashMap;
use std::fmt;

use crate::cost_firewall::{self, CostFirewallError, FreeOnlyPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostMode {
    #[default]
    FreeOnly,

    BringYourOwnKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Code,

    Chat,

    Reasoning,

    Vision,
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskKind::Code => write!(f, "code"),
            TaskKind::Chat => write!(f, "chat"),
            TaskKind::Reasoning => write!(f, "reasoning"),
            TaskKind::Vision => write!(f, "vision"),
        }
    }
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(task: &str) -> TaskKind {
        let t = task.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| t.contains(w));

        if has(&[
            "image",
            "screenshot",
            "picture",
            "photo",
            "diagram",
            "visual",
            "ocr",
            "describe this image",
            "what do you see",
        ]) {
            return TaskKind::Vision;
        }
        if has(&[
            "prove",
            "proof",
            "theorem",
            "algorithm complexity",
            "design a system",
            "architecture",
            "trade-off",
            "tradeoff",
            "step by step",
            "multi-step",
            "plan",
            "strategy",
            "analyze deeply",
            "root cause",
        ]) {
            return TaskKind::Reasoning;
        }
        if has(&[
            "code",
            "function",
            "bug",
            "fix",
            "refactor",
            "compile",
            "test",
            "cargo",
            "rust",
            "python",
            "javascript",
            "typescript",
            ".rs",
            ".py",
            ".js",
            ".ts",
            "pull request",
            "diff",
            "implement",
        ]) {
            return TaskKind::Code;
        }
        TaskKind::Chat
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: String,

    pub base_url: String,

    pub kinds: Vec<TaskKind>,

    pub priority: u32,
}

impl ModelEntry {
    pub fn new(
        id: impl Into<String>,
        base_url: impl Into<String>,
        kinds: Vec<TaskKind>,
        priority: u32,
    ) -> Self {
        Self {
            id: id.into(),
            base_url: base_url.into(),
            kinds,
            priority,
        }
    }

    pub fn handles(&self, kind: TaskKind) -> bool {
        self.kinds.contains(&kind)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    models: Vec<ModelEntry>,
}

impl ModelRegistry {
    pub fn verified_free() -> Self {
        let all_kinds = vec![TaskKind::Code, TaskKind::Chat, TaskKind::Reasoning];
        // Single endpoint: model + base URL only, no provider names.
        // deepseek-v4.1-flash is the default model, the -uncensored build
        // is the fallback; both are served by the same endpoint.
        let models = vec![
            ModelEntry::new(
                "deepseek-v4.1-flash",
                "https://api.experientiallabs.ai/api/v1",
                all_kinds.clone(),
                10,
            ),
            ModelEntry::new(
                "deepseek-v4.1-flash-uncensored",
                "https://api.experientiallabs.ai/api/v1",
                all_kinds,
                20,
            ),
        ];
        let registry = Self { models };
        debug_assert!(registry.all_free(), "registry contains a non-free model id");
        registry
    }

    pub fn custom(models: Vec<ModelEntry>) -> Self {
        Self { models }
    }

    fn all_free(&self) -> bool {
        self.models
            .iter()
            .all(|m| cost_firewall::is_free_model(&m.id))
    }

    pub fn models(&self) -> &[ModelEntry] {
        &self.models
    }

    pub fn for_kind(&self, kind: TaskKind) -> Vec<&ModelEntry> {
        let mut out: Vec<&ModelEntry> = self.models.iter().filter(|m| m.handles(kind)).collect();
        out.sort_by_key(|m| m.priority);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthState {
    #[default]
    Healthy,

    Degraded,

    Dead,
}

#[derive(Debug, Clone, Default)]
pub struct HealthTracker {
    failures: HashMap<String, u32>,
}

impl HealthTracker {
    pub const DEAD_AFTER: u32 = 3;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_success(&mut self, model_id: &str) {
        self.failures.remove(model_id);
    }

    pub fn record_failure(&mut self, model_id: &str) -> HealthState {
        let n = self.failures.entry(model_id.to_string()).or_insert(0);
        *n = n.saturating_add(1);
        self.state(model_id)
    }

    pub fn state(&self, model_id: &str) -> HealthState {
        match self.failures.get(model_id).copied().unwrap_or(0) {
            0 => HealthState::Healthy,
            n if n >= Self::DEAD_AFTER => HealthState::Dead,
            _ => HealthState::Degraded,
        }
    }

    pub fn reset(&mut self) {
        self.failures.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    pub kind: TaskKind,
    pub primary: ModelEntry,
    pub fallbacks: Vec<ModelEntry>,
}

impl RoutePlan {
    pub fn attempt_order(&self) -> Vec<&ModelEntry> {
        std::iter::once(&self.primary)
            .chain(self.fallbacks.iter())
            .collect()
    }

    pub fn model_ids(&self) -> Vec<&str> {
        self.attempt_order().iter().map(|m| m.id.as_str()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterError {
    NoModelAvailable {
        kind: TaskKind,
        mode: CostMode,
    },

    PaidModelBlocked {
        model: String,
        source: CostFirewallError,
    },
}

impl fmt::Display for RouterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouterError::NoModelAvailable { kind, mode } => write!(
                f,
                "smart router: no {kind} model available in {mode:?} mode"
            ),
            RouterError::PaidModelBlocked { model, source } => {
                write!(f, "smart router blocked '{model}': {source}")
            }
        }
    }
}

impl std::error::Error for RouterError {}

#[derive(Debug)]
pub struct SmartRouter {
    registry: ModelRegistry,
    health: HealthTracker,
    policy: FreeOnlyPolicy,
    mode: CostMode,
}

impl SmartRouter {
    pub fn free_only() -> Self {
        Self {
            registry: ModelRegistry::verified_free(),
            health: HealthTracker::new(),
            policy: FreeOnlyPolicy::default(),
            mode: CostMode::FreeOnly,
        }
    }

    pub fn with_registry(registry: ModelRegistry, mode: CostMode) -> Self {
        Self {
            registry,
            health: HealthTracker::new(),
            policy: FreeOnlyPolicy::default(),
            mode,
        }
    }

    pub fn mode(&self) -> CostMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: CostMode) {
        self.mode = mode;
    }

    pub fn route(&self, task: &str) -> Result<RoutePlan, RouterError> {
        let kind = TaskClassifier::classify(task);
        let mut candidates = self.registry.for_kind(kind);

        candidates.retain(|m| self.health.state(&m.id) != HealthState::Dead);
        candidates.sort_by_key(|m| (self.health.state(&m.id) != HealthState::Healthy, m.priority));

        let mut seen_ids = std::collections::HashSet::new();
        candidates.retain(|m| seen_ids.insert(m.id.clone()));

        if self.mode == CostMode::FreeOnly {
            for m in &candidates {
                if let Err(e) = self.policy.validate(&m.id) {
                    return Err(RouterError::PaidModelBlocked {
                        model: m.id.clone(),
                        source: e,
                    });
                }
            }
        }

        let mut iter = candidates.into_iter();
        match iter.next() {
            Some(primary) => Ok(RoutePlan {
                kind,
                primary: (*primary).clone(),
                fallbacks: iter.map(|m| (*m).clone()).collect(),
            }),
            None => Err(RouterError::NoModelAvailable {
                kind,
                mode: self.mode,
            }),
        }
    }

    pub fn record_success(&mut self, model_id: &str) {
        self.health.record_success(model_id);
    }

    pub fn record_failure(&mut self, model_id: &str) -> HealthState {
        self.health.record_failure(model_id)
    }

    pub fn health_of(&self, model_id: &str) -> HealthState {
        self.health.state(model_id)
    }

    pub fn reset_health(&mut self) {
        self.health.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_all_free() {
        let r = ModelRegistry::verified_free();
        assert!(!r.models().is_empty());
        for m in r.models() {
            assert!(
                cost_firewall::is_free_model(&m.id),
                "registry entry is not provably free: {}",
                m.id
            );
        }
    }

    #[test]
    fn classifier_code() {
        assert_eq!(
            TaskClassifier::classify("fix the null pointer bug in main.rs"),
            TaskKind::Code
        );
        assert_eq!(
            TaskClassifier::classify("refactor this function to be async"),
            TaskKind::Code
        );
        assert_eq!(
            TaskClassifier::classify("write a python script that parses logs"),
            TaskKind::Code
        );
    }

    #[test]
    fn classifier_reasoning() {
        assert_eq!(
            TaskClassifier::classify("prove that this algorithm is O(n log n)"),
            TaskKind::Reasoning
        );
        assert_eq!(
            TaskClassifier::classify("design a system architecture for 1M users, trade-offs?"),
            TaskKind::Reasoning
        );
    }

    #[test]
    fn classifier_vision_wins() {
        assert_eq!(
            TaskClassifier::classify("look at this screenshot and fix the bug in the code"),
            TaskKind::Vision
        );
    }

    #[test]
    fn classifier_chat_default() {
        assert_eq!(
            TaskClassifier::classify("hello, how are you today?"),
            TaskKind::Chat
        );
        assert_eq!(TaskClassifier::classify(""), TaskKind::Chat);
    }

    #[test]
    fn route_returns_priority_ordered_plan() {
        let router = SmartRouter::free_only();
        let plan = router.route("fix the bug").unwrap();
        assert_eq!(plan.kind, TaskKind::Code);

        let ids = plan.model_ids();
        assert!(ids.len() >= 2, "need primary + at least one fallback");
        assert_eq!(ids[0], "deepseek-v4.1-flash");

        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "duplicate model in plan: {id}");
        }
    }

    #[test]
    fn dead_models_are_skipped() {
        let mut router = SmartRouter::free_only();
        let first = router.route("fix the bug").unwrap();
        let dead_id = first.primary.id.clone();
        for _ in 0..HealthTracker::DEAD_AFTER {
            router.record_failure(&dead_id);
        }
        assert_eq!(router.health_of(&dead_id), HealthState::Dead);
        let second = router.route("fix the bug").unwrap();
        assert_ne!(second.primary.id, dead_id);
        assert!(
            !second.model_ids().contains(&dead_id.as_str()),
            "dead model must not appear anywhere in the plan"
        );
    }

    #[test]
    fn degraded_models_sink_to_end() {
        let mut router = SmartRouter::free_only();
        let first = router.route("hello").unwrap();
        let top_id = first.primary.id.clone();
        router.record_failure(&top_id);
        assert_eq!(router.health_of(&top_id), HealthState::Degraded);
        let second = router.route("hello").unwrap();
        assert_ne!(second.primary.id, top_id);

        assert!(second.model_ids().contains(&top_id.as_str()));
    }

    #[test]
    fn success_resets_failures() {
        let mut router = SmartRouter::free_only();
        let plan = router.route("hello").unwrap();
        let id = plan.primary.id.clone();
        router.record_failure(&id);
        router.record_failure(&id);
        assert_eq!(router.health_of(&id), HealthState::Degraded);
        router.record_success(&id);
        assert_eq!(router.health_of(&id), HealthState::Healthy);
    }

    #[test]
    fn free_only_blocks_paid_registry() {
        let registry = ModelRegistry::custom(vec![ModelEntry::new(
            "gpt-4o-paid",
            "https://example.com/v1",
            vec![TaskKind::Chat],
            1,
        )]);
        let router = SmartRouter::with_registry(registry, CostMode::FreeOnly);
        match router.route("hello") {
            Err(RouterError::PaidModelBlocked { model, .. }) => {
                assert_eq!(model, "gpt-4o-paid");
            }
            other => panic!("expected PaidModelBlocked, got {other:?}"),
        }
    }

    #[test]
    fn byok_allows_custom_models() {
        let registry = ModelRegistry::custom(vec![ModelEntry::new(
            "my-custom-model",
            "https://example.com/v1",
            vec![TaskKind::Chat],
            1,
        )]);
        let router = SmartRouter::with_registry(registry, CostMode::BringYourOwnKey);
        let plan = router.route("hello").unwrap();
        assert_eq!(plan.primary.id, "my-custom-model");
    }

    #[test]
    fn empty_registry_is_no_model_available() {
        let router = SmartRouter::with_registry(ModelRegistry::custom(vec![]), CostMode::FreeOnly);
        match router.route("hello") {
            Err(RouterError::NoModelAvailable { kind, mode }) => {
                assert_eq!(kind, TaskKind::Chat);
                assert_eq!(mode, CostMode::FreeOnly);
            }
            other => panic!("expected NoModelAvailable, got {other:?}"),
        }
    }

    #[test]
    fn all_dead_is_no_model_available() {
        let mut router = SmartRouter::free_only();

        let ids: Vec<String> = router
            .route("hello")
            .unwrap()
            .model_ids()
            .iter()
            .map(|s| s.to_string())
            .collect();
        for id in &ids {
            for _ in 0..HealthTracker::DEAD_AFTER {
                router.record_failure(id);
            }
        }
        assert!(router.route("hello").is_err());
    }

    #[test]
    fn set_mode_switches() {
        let mut router = SmartRouter::free_only();
        assert_eq!(router.mode(), CostMode::FreeOnly);
        router.set_mode(CostMode::BringYourOwnKey);
        assert_eq!(router.mode(), CostMode::BringYourOwnKey);
    }

    #[test]
    fn attempt_order_primary_first() {
        let router = SmartRouter::free_only();
        let plan = router.route("summarize this").unwrap();
        let order = plan.attempt_order();
        assert_eq!(order[0].id, plan.primary.id);
        assert_eq!(order.len(), 1 + plan.fallbacks.len());
    }
}
