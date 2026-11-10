//! Spawn the ZenCode CLI as a subprocess and stream its `stream-json` output.
//!
//! This module is the Desktop's only run path (DV-3 rewire). A run is
//! `zen -p --output-format stream-json <prompt>` as a child process; stdout
//! lines go through [`parse_stream_json`] and [`to_desktop_events`] and are
//! emitted live to a [`DesktopEventSink`]. Stderr is drained on a separate
//! task so a full stderr pipe can never stall the child.
//!
//! What stays Desktop-side (no CLI flag exists for it):
//!
//! * API key — the CLI reads it from the environment (`ZEN_API_KEY`, then
//!   `OPENAI_API_KEY`, then the provider env var;
//!   `crates/zen-cli/src/settings.rs::resolve_api_key`, CLI main 03eb057).
//!   The child inherits the Desktop process env; a key set on
//!   [`RunOptions::api_key`] is placed in the child's env as `ZEN_API_KEY`.
//!   The key is never passed via argv and never logged.
//! * Fallback models — when a run fails on the model/provider, the Desktop
//!   retries with the next fallback model (see [`is_model_provider_failure`]).
//! * `--max-output-tokens` is deliberately NOT passed: the CLI caps tokens
//!   per response, the Desktop caps the whole task. The Desktop counts
//!   output tokens from the stream (live estimate, then the authoritative
//!   usage on the `result` line) and stops the child at the total.
//! * `max_input_tokens` is estimated before spawn (prompt + system prompt,
//!   ~1 token per 4 chars) and checked again from the `result` line.
//! * `max_tool_calls`, the `files` list and the 3 convergence fields are
//!   enforced by observing the event stream and stopping the child.
//!
//! Stop/kill/timeout (Leader decision, tin 9078): SIGINT first (Unix) /
//! Ctrl-C (Windows, child in its own process group), wait 5s, then SIGTERM
//! / TerminateProcess, wait 5s, then SIGKILL. Exit 130 is a user stop,
//! 143 is kill/timeout.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Notify};

use crate::budget::{estimate_tokens, BudgetEnforcer};
use crate::claims::{ClaimObserver, ClaimRegistry};
use crate::convergence::ConvergenceMonitor;
use crate::cost_firewall::FreeOnlyPolicy;
use crate::events::{DesktopEvent, DesktopEventSink};
use crate::scope::{emit_scope_declared, needs_approval, ScopeObserver};
use crate::stream_json::{
    interpret_exit_code, parse_stream_json, to_desktop_events, CliExit, CliLine,
};
use crate::{telemetry, RunOptions};

/// Where to get the CLI when it is not on PATH (CLI README, `## Install`).
pub const INSTALL_HINT: &str = "install the ZenCode CLI: https://github.com/7enlab/zencode#install";

/// Keep only the tail of stderr (error detail on exit 1, never a full dump).
const STDERR_TAIL_CAP: usize = 32 * 1024;

/// Grace period between stop-escalation steps (SIGINT -> SIGTERM -> SIGKILL).
const STOP_GRACE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Step 1 — resolve the binary
// ---------------------------------------------------------------------------

/// Find the `zen` binary: `ZEN_BIN` (dev/test override) first, then PATH.
/// No sidecar yet; a missing binary is a clear one-line error.
pub fn resolve_zen_binary() -> anyhow::Result<PathBuf> {
    let zen_bin = std::env::var("ZEN_BIN")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let path_var = std::env::var("PATH").ok();
    resolve_zen_binary_from(zen_bin.as_deref(), path_var.as_deref())
}

/// Pure form of [`resolve_zen_binary`] (takes the env values as arguments so
/// tests do not have to mutate the process environment).
pub fn resolve_zen_binary_from(
    zen_bin: Option<&str>,
    path_var: Option<&str>,
) -> anyhow::Result<PathBuf> {
    if let Some(raw) = zen_bin {
        let p = PathBuf::from(raw);
        if !p.is_file() {
            anyhow::bail!("ZEN_BIN points at {raw:?}, which is not a file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p)
                .map(|m| m.permissions().mode())
                .unwrap_or(0);
            if mode & 0o111 == 0 {
                anyhow::bail!("ZEN_BIN points at {raw:?}, which is not executable");
            }
        }
        return Ok(p);
    }
    let path_var = path_var.unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        #[cfg(windows)]
        let candidates = [dir.join("zen.exe"), dir.join("zen")];
        #[cfg(not(windows))]
        let candidates = [dir.join("zen")];
        for c in candidates {
            if !c.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&c)
                    .map(|m| m.permissions().mode())
                    .unwrap_or(0);
                if mode & 0o111 == 0 {
                    continue;
                }
            }
            return Ok(c);
        }
    }
    anyhow::bail!("zen CLI binary not found on PATH; {INSTALL_HINT} or set ZEN_BIN to its path")
}

// ---------------------------------------------------------------------------
// Step 5 — map RunOptions to real CLI flags
// ---------------------------------------------------------------------------

/// Build the CLI argv for one run. Every flag here was read from
/// `zen --help` on CLI main 03eb057; no flag is invented.
///
/// * `--session-id` takes a fresh UUID per attempt: the CLI requires a UUID
///   (`parse_session_uuid`) and the Desktop session id (`YYYY-MM-DD-<hash>`)
///   is not one, so the Desktop id is kept for the Desktop session store and
///   the CLI id is returned in [`CliRunOutcome::cli_session_id`].
/// * `--add-dir` uses the `--add-dir=<dir>` form per directory: the
///   space form (`--add-dir a b`) is greedy (`num_args = 1..`) and would
///   swallow the prompt positional that follows it.
/// * `--max-output-tokens` is NOT passed (different meaning, see module docs).
/// * `dangerous_allow_all: true` maps to `--permission-mode bypassPermissions`;
///   `false` omits the flag (CLI default mode).
pub fn build_argv(opts: &RunOptions, cli_session_id: &str, prompt: &str) -> Vec<String> {
    let mut argv = vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--model".to_string(),
        opts.model.clone(),
        "--base-url".to_string(),
        opts.base_url.clone(),
        "--max-turns".to_string(),
        opts.max_turns.to_string(),
        "--session-id".to_string(),
        cli_session_id.to_string(),
    ];
    if !opts.system_prompt.trim().is_empty() {
        argv.push("--system-prompt".to_string());
        argv.push(opts.system_prompt.clone());
    }
    if let Some(scope) = &opts.scope {
        for d in &scope.dirs {
            argv.push(format!("--add-dir={}", d.display()));
        }
    }
    if opts.dangerous_allow_all {
        argv.push("--permission-mode".to_string());
        argv.push("bypassPermissions".to_string());
    }
    argv.push(prompt.to_string());
    argv
}

// ---------------------------------------------------------------------------
// Outcome + stop handle
// ---------------------------------------------------------------------------

/// Outcome of one CLI subprocess run.
#[derive(Debug, Clone)]
pub struct CliRunOutcome {
    pub exit: CliExit,
    pub exit_code: i32,
    /// False when the run was stopped, timed out, errored, or ended without
    /// a successful `result` line.
    pub success: bool,
    /// Final text: the CLI `result` line, or the stop/timeout/error reason.
    pub result: String,
    pub num_turns: u64,
    pub total_cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: String,
    pub duration_ms: u64,
    /// The UUID passed as `--session-id` for this attempt.
    pub cli_session_id: String,
    /// Why the run stopped early: "user stop", "timeout", "budget: …",
    /// "scope: …", or the convergence guard reason. `None` when the CLI
    /// finished on its own.
    pub stop_reason: Option<String>,
    pub timed_out: bool,
    /// Tail of the child's stderr (capped, for error detail).
    pub stderr_tail: String,
}

/// Handle for stopping a running child (UI Stop button / timeout).
/// The run loop polls both the flag and the notification, so a stop
/// requested while the loop is busy processing a line is not lost.
#[derive(Debug, Default)]
pub struct CliRunHandle {
    pid: Mutex<Option<u32>>,
    stop_requested: AtomicBool,
    stop_reason: Mutex<Option<String>>,
    notify: Notify,
}

impl CliRunHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Ask the run loop to stop the child. `reason` is surfaced in the
    /// outcome (`"user stop"`, `"timeout"`, …).
    pub fn request_stop(&self, reason: &str) {
        *self.stop_reason.lock().unwrap() = Some(reason.to_string());
        self.stop_requested.store(true, Ordering::SeqCst);
        // notify_one stores a permit when nobody is waiting, so a stop
        // requested between loop iterations still wakes the next select.
        self.notify.notify_one();
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::SeqCst)
    }

    pub fn stop_reason(&self) -> Option<String> {
        self.stop_reason.lock().unwrap().clone()
    }

    async fn notified(&self) {
        self.notify.notified().await;
    }

    fn set_pid(&self, pid: u32) {
        *self.pid.lock().unwrap() = Some(pid);
    }

    /// The child pid once spawned (`None` before spawn). Tests use it to
    /// prove no process is left behind after a stop.
    pub fn pid(&self) -> Option<u32> {
        *self.pid.lock().unwrap()
    }
}

// ---------------------------------------------------------------------------
// Step 6 — stop / kill / timeout
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn send_interrupt(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGINT);
    }
}

#[cfg(unix)]
fn send_terminate(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

#[cfg(windows)]
fn send_interrupt(pid: u32) {
    // The child was spawned in its own process group
    // (CREATE_NEW_PROCESS_GROUP), so Ctrl-C goes to the child group only.
    // NOT RUN on Windows by this seat (Linux box); the Leader runs the
    // Windows check in step 8.
    unsafe {
        type Bool = i32;
        #[link(name = "kernel32")]
        extern "system" {
            fn GenerateConsoleCtrlEvent(dw_ctrl_event: u32, dw_process_group_id: u32) -> Bool;
        }
        const CTRL_C_EVENT: u32 = 0;
        if GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid) == 0 {
            eprintln!("cli_spawn: GenerateConsoleCtrlEvent failed for pid {pid}");
        }
    }
}

#[cfg(windows)]
fn send_terminate(_pid: u32) {
    // TerminateProcess is Child::kill below; nothing to do here.
}

/// SIGINT/Ctrl-C, wait 5s, SIGTERM/TerminateProcess, wait 5s, SIGKILL.
async fn escalate_stop(child: &mut Child, pid: u32) {
    send_interrupt(pid);
    if wait_exit(child, STOP_GRACE).await {
        return;
    }
    send_terminate(pid);
    if wait_exit(child, STOP_GRACE).await {
        return;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn wait_exit(child: &mut Child, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(_) => return true,
        }
    }
}

/// Map a wait status to a contract exit code. A child killed by a signal
/// reports no code; the signal number maps back (SIGINT -> 130,
/// SIGTERM -> 143) so a stop is never misread as an unknown code.
fn exit_code_of(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        match status.signal() {
            Some(2) => 130,
            Some(15) => 143,
            Some(s) => -s,
            None => -1,
        }
    }
    #[cfg(not(unix))]
    -1
}

// ---------------------------------------------------------------------------
// Run loop
// ---------------------------------------------------------------------------

enum StreamMsg {
    Line(Box<CliLine>),
    ParseError(String),
}

struct SessionEndData {
    success: bool,
    result: String,
    num_turns: u64,
    total_cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
    model: String,
}

struct RunCtx {
    sink: Arc<dyn DesktopEventSink>,
    claims: ClaimObserver,
    budget: Option<Arc<BudgetEnforcer>>,
    convergence: Option<Arc<ConvergenceMonitor>>,
    scope: Option<ScopeObserver>,
    session_end_emitted: bool,
    last_session_end: Option<SessionEndData>,
}

impl RunCtx {
    /// Project one parsed line to Desktop events, fill claim counts on
    /// `SessionEnd`, emit live, then run the observers. Returns a stop
    /// reason when a limit is hit (the caller stops the child).
    fn process_line(&mut self, line: &CliLine) -> Option<String> {
        for mut ev in to_desktop_events(line) {
            if let DesktopEvent::SessionEnd {
                claims_verified,
                claims_unverified,
                claims_contradicted,
                ..
            } = &mut ev
            {
                let counts = self.claims.registry().counts();
                *claims_verified = counts.verified;
                *claims_unverified = counts.unverified;
                *claims_contradicted = counts.contradicted;
                self.session_end_emitted = true;
                if let DesktopEvent::SessionEnd {
                    success,
                    result,
                    num_turns,
                    total_cost_usd,
                    input_tokens,
                    output_tokens,
                    model,
                    ..
                } = &ev
                {
                    self.last_session_end = Some(SessionEndData {
                        success: *success,
                        result: result.clone(),
                        num_turns: *num_turns,
                        total_cost_usd: *total_cost_usd,
                        input_tokens: *input_tokens,
                        output_tokens: *output_tokens,
                        model: model.clone(),
                    });
                }
            }
            self.sink.emit_desktop(ev.clone());
            self.claims.observe(&ev);
            if let Some(budget) = &self.budget {
                if let Err(e) = budget.observe(&ev) {
                    return Some(format!("budget: {e}"));
                }
            }
            if let Some(monitor) = &self.convergence {
                if let Some(reason) = monitor.observe(&ev) {
                    return Some(reason);
                }
            }
            if let Some(scope) = &self.scope {
                if let Some(v) = scope.observe(&ev) {
                    return Some(format!(
                        "scope: {} '{}' is outside the declared scope ({})",
                        v.tool, v.target, v.reason
                    ));
                }
            }
        }
        None
    }
}

/// Rough input-token estimate for the pre-spawn check: ~1 token per 4 chars
/// of prompt + system prompt. The real count comes from the `result` line.
pub fn estimate_input_tokens(prompt: &str, system_prompt: &str) -> u64 {
    estimate_tokens(prompt) + estimate_tokens(system_prompt)
}

/// Run one attempt with the given model. Public entry is [`run_cli_task`],
/// which retries on model/provider failure with the fallback models.
async fn run_attempt(
    opts: &RunOptions,
    prompt: &str,
    sink: &Arc<dyn DesktopEventSink>,
    handle: &Arc<CliRunHandle>,
    binary: &Path,
) -> anyhow::Result<CliRunOutcome> {
    if prompt.trim().is_empty() {
        anyhow::bail!("empty task");
    }
    if opts.base_url.trim().is_empty() {
        anyhow::bail!("base_url is empty");
    }
    if !opts.base_url.starts_with("http://") && !opts.base_url.starts_with("https://") {
        anyhow::bail!(
            "base_url must start with http:// or https://: {}",
            opts.base_url
        );
    }
    if opts.model.trim().is_empty() {
        anyhow::bail!("model is empty");
    }
    FreeOnlyPolicy::default()
        .validate_all(&opts.model, &opts.fallback_models)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let guard = telemetry::TelemetryGuard::from_base_url(&opts.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    telemetry::emit_telemetry_ready(sink, &guard);
    if !opts.cwd.is_dir() {
        anyhow::bail!("cwd is not a directory: {}", opts.cwd.display());
    }
    if let Some(decl) = &opts.scope {
        let reasons = needs_approval(decl, &opts.cwd);
        if !reasons.is_empty() {
            let why: Vec<String> = reasons.iter().map(|r| r.message()).collect();
            anyhow::bail!(
                "scope declaration needs explicit approval before starting:\n- {}",
                why.join("\n- ")
            );
        }
        emit_scope_declared(sink, decl);
    }
    if let Some(policy) = &opts.convergence {
        policy.validate()?;
    }
    if let Some(budget) = &opts.budget {
        let est = estimate_input_tokens(prompt, &opts.system_prompt);
        if est > budget.max_input_tokens {
            anyhow::bail!(
                "prompt is too large for the input budget: estimated {est} input tokens, limit {}",
                budget.max_input_tokens
            );
        }
    }

    let started = Instant::now();
    let cli_session_id = uuid::Uuid::new_v4().to_string();
    let argv = build_argv(opts, &cli_session_id, prompt);

    let mut std_cmd = std::process::Command::new(binary);
    std_cmd
        .args(&argv)
        .current_dir(&opts.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Own process group so Ctrl-C in send_interrupt hits only the child.
        std_cmd.creation_flags(0x0000_0200);
    }
    if let Some(key) = &opts.api_key {
        std_cmd.env("ZEN_API_KEY", key);
    }
    let mut child = Command::from(std_cmd)
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", binary.display()))?;
    let pid = child.id().unwrap_or(0);
    handle.set_pid(pid);

    let mut ctx = RunCtx {
        sink: sink.clone(),
        claims: ClaimObserver::new(Arc::new(ClaimRegistry::new())),
        budget: opts
            .budget
            .clone()
            .map(|b| BudgetEnforcer::new(b, sink.clone())),
        convergence: match &opts.convergence {
            Some(policy) => Some(ConvergenceMonitor::new(policy.clone(), sink.clone())?),
            None => None,
        },
        scope: opts
            .scope
            .clone()
            .map(|decl| ScopeObserver::new(opts.cwd.clone(), decl, sink.clone())),
        session_end_emitted: false,
        last_session_end: None,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<StreamMsg>();
    let stdout = child.stdout.take().expect("stdout piped");
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match parse_stream_json(&line) {
                Ok(parsed) => {
                    for l in parsed {
                        if tx.send(StreamMsg::Line(Box::new(l))).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    if tx.send(StreamMsg::ParseError(e.to_string())).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_end(&mut buf).await;
        let text = String::from_utf8_lossy(&buf).into_owned();
        if text.len() > STDERR_TAIL_CAP {
            text[text.len() - STDERR_TAIL_CAP..].to_string()
        } else {
            text
        }
    });

    // No deadline when the budget is unlimited: adding u64::MAX seconds to
    // an Instant overflows tokio's clock.
    let deadline = opts
        .budget
        .as_ref()
        .filter(|b| b.max_duration_secs < u64::MAX)
        .map(|b| tokio::time::Instant::now() + Duration::from_secs(b.max_duration_secs));
    let deadline_sleep = async move {
        match deadline {
            Some(d) => tokio::time::sleep_until(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(deadline_sleep);

    let mut pending_stop: Option<String> = None;
    let mut timed_out = false;
    loop {
        tokio::select! {
            biased;
            _ = handle.notified() => {
                pending_stop = Some(
                    handle
                        .stop_reason()
                        .unwrap_or_else(|| "user stop".to_string()),
                );
                break;
            }
            _ = &mut deadline_sleep => {
                timed_out = true;
                pending_stop = Some("timeout".to_string());
                break;
            }
            msg = rx.recv() => {
                match msg {
                    Some(StreamMsg::Line(line)) => {
                        if let Some(reason) = ctx.process_line(&line) {
                            pending_stop = Some(reason);
                            break;
                        }
                    }
                    Some(StreamMsg::ParseError(e)) => {
                        sink.emit_desktop(DesktopEvent::SystemMessage {
                            event: "cli_stream:parse_error".to_string(),
                            text: format!("unparseable stream-json line from the CLI: {e}"),
                        });
                    }
                    None => break, // stdout closed: the child is done
                }
            }
        }
        if handle.stop_requested() && pending_stop.is_none() {
            pending_stop = Some(
                handle
                    .stop_reason()
                    .unwrap_or_else(|| "user stop".to_string()),
            );
            break;
        }
    }

    if let Some(reason) = &pending_stop {
        handle.request_stop(reason);
        escalate_stop(&mut child, pid).await;
    }
    // Drain whatever the child flushed before exiting (on SIGINT the CLI
    // emits a final `result` line, which must still be processed).
    let _ = reader.await;
    while let Ok(msg) = rx.try_recv() {
        match msg {
            StreamMsg::Line(line) => {
                let _ = ctx.process_line(&line);
            }
            StreamMsg::ParseError(e) => {
                sink.emit_desktop(DesktopEvent::SystemMessage {
                    event: "cli_stream:parse_error".to_string(),
                    text: format!("unparseable stream-json line from the CLI: {e}"),
                });
            }
        }
    }

    let status = child.wait().await?;
    let stderr_tail = stderr_task.await.unwrap_or_default();
    let exit_code = exit_code_of(&status);
    let exit = interpret_exit_code(exit_code);
    let duration_ms = started.elapsed().as_millis() as u64;

    // The Desktop stopped this run (user, timeout, or an observer limit):
    // the outcome carries the stop reason even when the CLI also emitted
    // a `result` line (e.g. "Interrupted by user" after SIGINT).
    let stopped_early = pending_stop.is_some();
    let stop_text = || {
        if timed_out {
            format!(
                "run timed out after {}s",
                opts.budget
                    .as_ref()
                    .map(|b| b.max_duration_secs)
                    .unwrap_or(0)
            )
        } else {
            pending_stop.clone().unwrap_or_default()
        }
    };

    let (success, result, num_turns, total_cost_usd, input_tokens, output_tokens, model) =
        match ctx.last_session_end {
            Some(end) if !stopped_early => (
                end.success,
                end.result,
                end.num_turns,
                end.total_cost_usd,
                end.input_tokens,
                end.output_tokens,
                end.model,
            ),
            Some(end) => (
                false,
                stop_text(),
                end.num_turns,
                end.total_cost_usd,
                end.input_tokens,
                end.output_tokens,
                end.model,
            ),
            None => {
                let (used_in, used_out, _, _) = ctx
                    .budget
                    .as_ref()
                    .map(|b| b.snapshot())
                    .unwrap_or((0, 0, 0, 0));
                let text = if stopped_early {
                    stop_text()
                } else {
                    match exit {
                        CliExit::Success => "CLI exited 0 without a result line".to_string(),
                        CliExit::Error => {
                            let first = stderr_tail.lines().next().unwrap_or("").trim();
                            if first.is_empty() {
                                "CLI exited 1 (see stderr)".to_string()
                            } else {
                                format!("CLI exited 1: {first}")
                            }
                        }
                        CliExit::Interrupted => "Interrupted by user".to_string(),
                        CliExit::Terminated => "Terminated".to_string(),
                        CliExit::Other(c) => format!("CLI exited with code {c}"),
                    }
                };
                (false, text, 0, 0.0, used_in, used_out, opts.model.clone())
            }
        };

    // SessionEnd is emitted exactly once per run: from the `result` line
    // when the CLI produced one, synthesized here otherwise.
    if !ctx.session_end_emitted {
        let counts = ctx.claims.registry().counts();
        sink.emit_desktop(DesktopEvent::SessionEnd {
            success,
            result: result.clone(),
            num_turns,
            total_cost_usd,
            input_tokens,
            output_tokens,
            model: model.clone(),
            duration_ms,
            claims_verified: counts.verified,
            claims_unverified: counts.unverified,
            claims_contradicted: counts.contradicted,
        });
    }

    Ok(CliRunOutcome {
        exit,
        exit_code,
        success,
        result,
        num_turns,
        total_cost_usd,
        input_tokens,
        output_tokens,
        model,
        duration_ms,
        cli_session_id,
        stop_reason: pending_stop,
        timed_out,
        stderr_tail,
    })
}

/// Does this failed outcome look like a model/provider failure (wrong key,
/// unknown model, rate limit, provider outage)? Only then is a fallback
/// model tried; any other failure is returned as-is.
fn is_model_provider_failure(outcome: &CliRunOutcome) -> bool {
    if outcome.exit != CliExit::Error {
        return false;
    }
    const MARKERS: &[&str] = &[
        "401",
        "402",
        "429",
        "rate limit",
        "rate_limit",
        "unauthorized",
        "authentication",
        "api key",
        "api_key",
        "unknown model",
        "model not found",
        "does not exist",
        "overloaded",
        "provider error",
        "connection refused",
        "connection reset",
    ];
    let hay = format!("{}\n{}", outcome.stderr_tail, outcome.result).to_lowercase();
    MARKERS.iter().any(|m| hay.contains(m))
}

/// Run a prompt through the CLI subprocess (steps 1-6). Resolves the binary,
/// then tries the configured model and, on a model/provider failure, each
/// fallback model in order. The child inherits the Desktop process env, so
/// the CLI reads its API key the same way it always does (see module docs).
pub async fn run_cli_task(
    opts: &RunOptions,
    prompt: &str,
    sink: Arc<dyn DesktopEventSink>,
    handle: &Arc<CliRunHandle>,
) -> anyhow::Result<CliRunOutcome> {
    run_cli_task_with_binary(opts, prompt, sink, handle, None).await
}

/// [`run_cli_task`] with an explicit binary (tests point it at a fake `zen`
/// script; production passes `None` and resolves via `ZEN_BIN` / PATH).
pub async fn run_cli_task_with_binary(
    opts: &RunOptions,
    prompt: &str,
    sink: Arc<dyn DesktopEventSink>,
    handle: &Arc<CliRunHandle>,
    binary: Option<PathBuf>,
) -> anyhow::Result<CliRunOutcome> {
    let binary = match binary {
        Some(b) => b,
        None => resolve_zen_binary()?,
    };
    sink.emit_desktop(DesktopEvent::SystemMessage {
        event: "cli_spawn:binary".to_string(),
        text: format!("resolved zen binary: {}", binary.display()),
    });
    let mut models = vec![opts.model.clone()];
    models.extend(opts.fallback_models.iter().cloned());
    let mut last: Option<CliRunOutcome> = None;
    for (i, model) in models.iter().enumerate() {
        let mut attempt_opts = opts.clone();
        attempt_opts.model = model.clone();
        let outcome = run_attempt(&attempt_opts, prompt, &sink, handle, &binary).await?;
        let failed_on_provider = !outcome.success && is_model_provider_failure(&outcome);
        last = Some(outcome);
        if handle.stop_requested() {
            break;
        }
        if !failed_on_provider || i + 1 == models.len() {
            break;
        }
        let next = &models[i + 1];
        sink.emit_desktop(DesktopEvent::SystemMessage {
            event: "cli_fallback_retry".to_string(),
            text: format!(
                "model '{model}' failed on the provider; retrying with fallback model '{next}'"
            ),
        });
    }
    Ok(last.expect("at least one attempt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_opts() -> RunOptions {
        let mut opts = RunOptions::new(
            "http://127.0.0.1:18123/v1",
            "mock-model:free",
            std::env::temp_dir(),
        );
        opts.max_turns = 3;
        opts.system_prompt = "Be brief.".to_string();
        opts.scope = Some(crate::scope::ScopeDecl {
            files: vec![],
            dirs: vec![std::env::temp_dir()],
            description: "temp dir".to_string(),
        });
        opts
    }

    #[test]
    fn resolve_prefers_zen_bin_override() {
        let exe = std::env::current_exe().unwrap();
        let found = resolve_zen_binary_from(Some(exe.to_str().unwrap()), Some("/usr/bin")).unwrap();
        assert_eq!(found, exe);
    }

    #[test]
    fn resolve_finds_zen_on_path() {
        let dir = std::env::temp_dir().join("zencode-resolve-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("zen");
        {
            let mut f = std::fs::File::create(&fake).unwrap();
            f.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = resolve_zen_binary_from(None, Some(dir.to_str().unwrap())).unwrap();
        assert_eq!(found, fake);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_missing_binary_names_the_install() {
        let err = resolve_zen_binary_from(None, Some("/nonexistent-dir-xyz")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not found on PATH"), "{msg}");
        assert!(msg.contains("7enlab/zencode"), "{msg}");
    }

    #[test]
    fn resolve_rejects_non_file_zen_bin() {
        let err = resolve_zen_binary_from(Some("/nonexistent-dir-xyz/zen"), Some("")).unwrap_err();
        assert!(err.to_string().contains("not a file"), "{err}");
    }

    #[test]
    fn argv_maps_all_flag_groups() {
        let mut opts = sample_opts();
        opts.fallback_models = vec!["fallback:free".to_string()];
        opts.dangerous_allow_all = true;
        let argv = build_argv(
            &opts,
            "00000000-0000-4000-8000-000000000001",
            "do the thing",
        );
        assert_eq!(&argv[0..3], ["-p", "--output-format", "stream-json"]);
        assert!(argv.contains(&"--model".to_string()));
        assert!(argv.contains(&"mock-model:free".to_string()));
        assert!(argv.contains(&"--base-url".to_string()));
        assert!(argv.contains(&"http://127.0.0.1:18123/v1".to_string()));
        assert!(argv.contains(&"--max-turns".to_string()));
        assert!(argv.contains(&"3".to_string()));
        assert!(argv.contains(&"--system-prompt".to_string()));
        assert!(argv.contains(&"Be brief.".to_string()));
        assert!(argv.contains(&"--session-id".to_string()));
        assert!(argv.contains(&"00000000-0000-4000-8000-000000000001".to_string()));
        assert!(argv.iter().any(|a| a.starts_with("--add-dir=")));
        assert!(argv.contains(&"--permission-mode".to_string()));
        assert!(argv.contains(&"bypassPermissions".to_string()));
        // No --max-output-tokens (different meaning), no api key anywhere.
        assert!(!argv.iter().any(|a| a.contains("max-output-tokens")));
        assert!(!argv.iter().any(|a| a.contains("api_key")));
        // The prompt is the last argv element (never swallowed by --add-dir).
        assert_eq!(argv.last().unwrap(), "do the thing");
    }

    #[test]
    fn argv_omits_bypass_when_not_dangerous() {
        let opts = sample_opts();
        let argv = build_argv(&opts, "00000000-0000-4000-8000-000000000002", "hi");
        assert!(!argv.contains(&"--permission-mode".to_string()));
        assert!(!argv.contains(&"bypassPermissions".to_string()));
    }

    #[test]
    fn argv_skips_empty_system_prompt() {
        let mut opts = sample_opts();
        opts.system_prompt = String::new();
        let argv = build_argv(&opts, "00000000-0000-4000-8000-000000000003", "hi");
        assert!(!argv.contains(&"--system-prompt".to_string()));
    }

    #[test]
    fn provider_failure_matcher() {
        let failed = |stderr: &str| CliRunOutcome {
            exit: CliExit::Error,
            exit_code: 1,
            success: false,
            result: String::new(),
            num_turns: 0,
            total_cost_usd: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            model: "m:free".to_string(),
            duration_ms: 0,
            cli_session_id: String::new(),
            stop_reason: None,
            timed_out: false,
            stderr_tail: stderr.to_string(),
        };
        assert!(is_model_provider_failure(&failed(
            "error: HTTP 429: rate limit exceeded"
        )));
        assert!(is_model_provider_failure(&failed(
            "model 'x' needs its provider's key: set ZEN_API_KEY"
        )));
        assert!(!is_model_provider_failure(&failed(
            "error: prompt was empty"
        )));
        let ok = CliRunOutcome {
            exit: CliExit::Success,
            ..failed("429")
        };
        assert!(!is_model_provider_failure(&ok));
    }

    #[test]
    fn input_estimate_is_chars_over_four() {
        assert_eq!(estimate_input_tokens("abcdefgh", ""), 2);
        assert_eq!(estimate_input_tokens("", "abcd"), 1);
    }
}
