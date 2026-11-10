pub mod budget;
pub mod checkpoints;
pub mod claims;
pub mod cli_spawn;
pub mod config_import;
pub mod convergence;
pub mod cost_firewall;
pub mod events;
pub mod ipc_bridge;
pub mod release_signing;
pub mod scope;
pub mod session_store;
pub mod smart_router;
pub mod sovereign;
pub mod stream_json;
pub mod telemetry;
pub mod timeline;

use std::path::PathBuf;
use std::sync::Arc;

use budget::TaskBudget;

pub use cli_spawn::{run_cli_task, CliRunHandle, CliRunOutcome};

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub base_url: String,

    /// API key for the model endpoint, if the caller holds one. It is never
    /// passed on the CLI argv: the child process inherits the Desktop env
    /// and the CLI reads `ZEN_API_KEY` / `OPENAI_API_KEY` / the provider
    /// env var from it (see `cli_spawn`). When set, it is placed in the
    /// child's env as `ZEN_API_KEY`.
    pub api_key: Option<String>,
    pub model: String,
    /// Fallback models, tried in order when a run fails on the
    /// model/provider. Kept Desktop-side; the CLI gets no `--settings`.
    pub fallback_models: Vec<String>,

    pub cwd: PathBuf,
    pub max_turns: usize,
    pub system_prompt: String,

    /// Budget enforced Desktop-side by observing the event stream
    /// (see `budget::BudgetEnforcer`): `max_input_tokens` is estimated
    /// before spawn and checked from the `result` line, `max_output_tokens`
    /// is counted from the stream, `max_tool_calls` from tool calls,
    /// `max_duration_secs` is the kill timeout.
    pub budget: Option<TaskBudget>,

    /// Scope enforced Desktop-side: `dirs` are passed as `--add-dir`,
    /// the `files` list and path-escape rules are checked by observing the
    /// stream (`scope::ScopeObserver`); the declaration itself must pass
    /// `needs_approval` before spawn.
    pub scope: Option<scope::ScopeDecl>,

    /// `true` maps to `--permission-mode bypassPermissions`, `false` to the
    /// CLI default mode.
    pub dangerous_allow_all: bool,

    pub telemetry_log: Option<Arc<telemetry::NetworkLog>>,

    /// Convergence policy enforced Desktop-side by observing the event
    /// stream (`convergence::ConvergenceMonitor`): on a detected loop the
    /// run is stopped and the pattern reported (no re-plan injection is
    /// possible across the process boundary).
    pub convergence: Option<convergence::ConvergencePolicy>,
}

impl RunOptions {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            model: model.into(),
            fallback_models: vec![],
            cwd: cwd.into(),
            max_turns: 12,
            system_prompt: "You are ZenCode, a concise coding agent. Use the tools to complete the task, then reply with a short summary.".into(),
            budget: None,
            scope: None,
            dangerous_allow_all: false,
            telemetry_log: None,
            convergence: None,
        }
    }
}
