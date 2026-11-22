use std::fmt;

pub const FREE_ALLOWLIST: &[&str] = &["deepseek-v4.1-flash", "deepseek-v4.1-flash-uncensored"];

pub const FREE_SUFFIX: &str = ":free";

pub const BLOCKED_MESSAGE: &str = "Zen cannot bill you - this is structural, not a setting.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostFirewallError {
    NonFreeModelBlocked { model: String },

    EmptyModelId,
}

impl fmt::Display for CostFirewallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostFirewallError::NonFreeModelBlocked { model } => write!(
                f,
                "Zen blocked model '{model}': only :free models are allowed. {BLOCKED_MESSAGE}"
            ),
            CostFirewallError::EmptyModelId => write!(
                f,
                "Zen blocked the request: no model id was provided. Only :free models are allowed. {BLOCKED_MESSAGE}"
            ),
        }
    }
}

impl std::error::Error for CostFirewallError {}

pub fn is_free_model(model: &str) -> bool {
    let m = model.trim();
    !m.is_empty() && (m.ends_with(FREE_SUFFIX) || FREE_ALLOWLIST.contains(&m))
}

pub fn validate_model(model: &str) -> Result<(), CostFirewallError> {
    if model.trim().is_empty() {
        return Err(CostFirewallError::EmptyModelId);
    }
    if is_free_model(model) {
        Ok(())
    } else {
        Err(CostFirewallError::NonFreeModelBlocked {
            model: model.trim().to_string(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct FreeOnlyPolicy {
    extra_allowlist: Vec<String>,
}

impl FreeOnlyPolicy {
    pub fn with_extra_allowlist(extra: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            extra_allowlist: extra.into_iter().map(Into::into).collect(),
        }
    }

    fn is_allowed(&self, model: &str) -> bool {
        let m = model.trim();
        !m.is_empty()
            && (m.ends_with(FREE_SUFFIX)
                || FREE_ALLOWLIST.contains(&m)
                || self.extra_allowlist.iter().any(|a| a == m))
    }

    pub fn validate(&self, model: &str) -> Result<(), CostFirewallError> {
        if model.trim().is_empty() {
            return Err(CostFirewallError::EmptyModelId);
        }
        if self.is_allowed(model) {
            Ok(())
        } else {
            Err(CostFirewallError::NonFreeModelBlocked {
                model: model.trim().to_string(),
            })
        }
    }

    pub fn validate_all(
        &self,
        primary: &str,
        fallbacks: &[String],
    ) -> Result<(), CostFirewallError> {
        self.validate(primary)?;
        for fb in fallbacks {
            self.validate(fb)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SpendMeter {
    tasks_run: u64,
}

impl SpendMeter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_task(&mut self) {
        self.tasks_run += 1;
    }

    pub fn tasks_run(&self) -> u64 {
        self.tasks_run
    }

    pub fn total_spent_usd(&self) -> f64 {
        0.0
    }

    pub fn cost_preview(&self, _task: &str) -> String {
        "$0.00".to_string()
    }

    pub fn summary(&self) -> String {
        format!(
            "Spent $0.00 across {} task(s). Zen cannot bill you.",
            self.tasks_run
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_suffix_models_pass() {
        for m in [
            "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
            "deepseek-v4.1-flash:free",
            "qwen3.8-flash:free",
        ] {
            assert!(validate_model(m).is_ok(), "should pass: {m}");
            assert!(is_free_model(m));
        }
    }

    #[test]
    fn allowlisted_models_pass() {
        assert!(validate_model("deepseek-v4.1-flash-uncensored").is_ok());
    }

    #[test]
    fn non_free_models_are_blocked() {
        for m in [
            "gpt-4",
            "gpt-4o",
            "claude-3-5-sonnet-20241022",
            "anthropic/claude-sonnet-4-6",
            "openai/gpt-4o-mini",
            "free",
        ] {
            let err = validate_model(m).unwrap_err();
            assert_eq!(
                err,
                CostFirewallError::NonFreeModelBlocked {
                    model: m.to_string()
                }
            );
        }
    }

    #[test]
    fn empty_model_id_is_blocked() {
        for m in ["", "   "] {
            assert_eq!(
                validate_model(m).unwrap_err(),
                CostFirewallError::EmptyModelId
            );
        }
    }

    #[test]
    fn blocked_message_is_loud_and_honest() {
        let msg = CostFirewallError::NonFreeModelBlocked {
            model: "gpt-4".into(),
        }
        .to_string();
        assert!(msg.contains("blocked model 'gpt-4'"), "{msg}");
        assert!(msg.contains("only :free models are allowed"), "{msg}");
        assert!(msg.contains("cannot bill you"), "{msg}");
        assert!(msg.contains("structural, not a setting"), "{msg}");
    }

    #[test]
    fn validation_is_pure_and_runs_before_any_network_use() {
        let policy = FreeOnlyPolicy::default();
        let err = policy
            .validate("gpt-4")
            .expect_err("must reject before any HTTP client is built");
        assert!(matches!(err, CostFirewallError::NonFreeModelBlocked { .. }));
    }

    #[test]
    fn validate_all_rejects_bad_fallback() {
        let policy = FreeOnlyPolicy::default();
        let err = policy
            .validate_all("deepseek-v4.1-flash:free", &["gpt-4".to_string()])
            .expect_err("fallback must be validated too");
        assert_eq!(
            err,
            CostFirewallError::NonFreeModelBlocked {
                model: "gpt-4".into()
            }
        );
    }

    #[test]
    fn validate_all_accepts_clean_lists() {
        let policy = FreeOnlyPolicy::default();
        policy
            .validate_all(
                "a:free",
                &[
                    "b:free".to_string(),
                    "deepseek-v4.1-flash-uncensored".to_string(),
                ],
            )
            .expect("all free");
    }

    #[test]
    fn extra_allowlist_extends_policy() {
        let policy = FreeOnlyPolicy::with_extra_allowlist(["my-lab/local-coder"]);
        assert!(policy.validate("my-lab/local-coder").is_ok());
        assert!(FreeOnlyPolicy::default()
            .validate("my-lab/local-coder")
            .is_err());
    }

    #[test]
    fn spend_meter_is_always_zero() {
        let mut meter = SpendMeter::new();
        assert_eq!(meter.total_spent_usd(), 0.0);
        assert_eq!(meter.cost_preview("refactor the auth module"), "$0.00");
        meter.record_task();
        meter.record_task();
        assert_eq!(meter.tasks_run(), 2);
        assert_eq!(meter.total_spent_usd(), 0.0);
        let summary = meter.summary();
        assert!(summary.contains("$0.00"), "{summary}");
        assert!(summary.contains('2'), "{summary}");
    }

    #[test]
    fn firewall_error_converts_to_anyhow() {
        let err: anyhow::Error = validate_model("gpt-4").unwrap_err().into();

        let msg = format!("{err:?}");
        assert!(msg.contains("blocked model 'gpt-4'"), "{msg}");
        assert!(
            err.downcast_ref::<CostFirewallError>()
                == Some(&CostFirewallError::NonFreeModelBlocked {
                    model: "gpt-4".into()
                }),
            "{msg}"
        );
    }
}
