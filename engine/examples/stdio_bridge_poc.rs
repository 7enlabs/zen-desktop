//! stdio NDJSON bridge PoC: spawn the real `zen` CLI as a child process and
//! speak stream-json v1 over stdio, timestamping every envelope.
//!
//! This is a measurement prototype, not the production run path. It reuses
//! the production pieces it measures: [`build_argv`] builds exactly the argv
//! `cli_spawn::run_cli_task` would build (`zen -p --output-format
//! stream-json --model … --base-url … --max-turns … --session-id … <prompt>`),
//! and each stdout line is parsed with [`parse_stream_json`] and projected
//! with [`to_desktop_events`]. The only PoC-only part is the timing harness:
//! every envelope is stamped with its arrival time (milliseconds since
//! spawn) and the run writes a machine-readable latency report.
//!
//! The CLI is pointed at a local mock LLM over HTTP (see
//! `docs/stdio-bridge-poc/mock_llm_stub.py`), so the run needs no network
//! and no real model key: `ZEN_API_KEY` is a dummy value, the model is
//! `mock-model`, and the stub returns one canned OpenAI-compatible reply.
//! Nothing here is faked: the child is the real CLI binary built from the
//! CLI repo, the bytes on stdout are its real stream-json output.
//!
//! Usage (see `docs/stdio-bridge-poc/run_poc.sh` for the full recipe):
//!
//! ```sh
//! POC_BASE_URL=http://127.0.0.1:18123/v1 POC_RUNS=5 \
//!   POC_OUT=/path/to/stdio-bridge-poc-run.json \
//!   cargo run --offline --locked --example stdio_bridge_poc -- /path/to/zen
//! ```
//!
//! Environment (all optional):
//!
//! * `POC_MODEL` (default `mock-model`), `POC_BASE_URL` (default
//!   `http://127.0.0.1:18123/v1`), `POC_PROMPT` (default `hello`),
//!   `POC_RUNS` (default 5), `POC_MAX_TURNS` (default 3).
//! * `POC_OUT` — path of the JSON latency report (default
//!   `stdio-bridge-poc-run.json` in the current directory).
//! * `POC_CLI_REV` — CLI repo revision of the binary, recorded verbatim in
//!   the report (the binary itself only reports `zen 0.1.0`).
//! * `POC_API_KEY` — dummy key placed in the child's env as `ZEN_API_KEY`
//!   (default `poc-dummy-key`); never printed or written to the report.
//! * `ZEN_BIN` — fallback binary resolution when no argv[1] is given (same
//!   rule as `cli_spawn::resolve_zen_binary`).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use zen_engine::cli_spawn::{build_argv, resolve_zen_binary};
use zen_engine::stream_json::{
    interpret_exit_code, parse_stream_json, to_desktop_events, CliExit, CliStreamEvent,
};
use zen_engine::RunOptions;

/// One envelope as it arrived on the child's stdout.
#[derive(Debug, Clone)]
struct EnvelopeTiming {
    seq: usize,
    /// Milliseconds from spawn to the complete line arriving.
    arrival_ms: f64,
    line_type: String,
    subtype: String,
    bytes: usize,
    desktop_events: usize,
}

#[derive(Debug, Clone)]
struct RunTiming {
    run: usize,
    session_id: String,
    exit_code: Option<i32>,
    exit: String,
    wall_ms: f64,
    first_envelope_ms: Option<f64>,
    result_envelope_ms: Option<f64>,
    envelopes: Vec<EnvelopeTiming>,
    stderr_tail: String,
    parse_errors: Vec<String>,
}

fn label_event(event: &CliStreamEvent) -> (String, String) {
    match event {
        CliStreamEvent::System(e) => ("system".to_string(), e.subtype.clone()),
        CliStreamEvent::Assistant { .. } => ("assistant".to_string(), String::new()),
        CliStreamEvent::User { .. } => ("user".to_string(), String::new()),
        CliStreamEvent::Result { subtype, .. } => ("result".to_string(), subtype.clone()),
        CliStreamEvent::StreamEvent { .. } => ("stream_event".to_string(), String::new()),
        CliStreamEvent::Unknown => ("unknown".to_string(), String::new()),
    }
}

fn percentile_sorted(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    // Nearest-rank percentile.
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[idx])
}

fn median_sorted(sorted: &[f64]) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
    }
}

fn stats(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "n": sorted.len(),
        "min_ms": sorted.first().copied(),
        "median_ms": median_sorted(&sorted),
        "p95_ms": percentile_sorted(&sorted, 95.0),
        "max_ms": sorted.last().copied(),
    })
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

async fn run_once(
    run: usize,
    binary: &std::path::Path,
    opts: &RunOptions,
    prompt: &str,
    scratch: &std::path::Path,
) -> anyhow::Result<RunTiming> {
    let home = scratch.join(format!("home-{run}"));
    let cwd = scratch.join(format!("cwd-{run}"));
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&cwd)?;
    let mut opts = opts.clone();
    opts.cwd = cwd;

    let session_id = uuid::Uuid::new_v4().to_string();
    let argv = build_argv(&opts, &session_id, prompt);
    let api_key = env_or("POC_API_KEY", "poc-dummy-key");

    let t0 = Instant::now();
    let mut child = Command::new(binary)
        .args(&argv)
        .current_dir(&opts.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("ZEN_API_KEY", &api_key)
        .env("ZEN_TRUST_FOLDER", "1")
        // The CLI prints an anonymous-usage notice to stderr by default;
        // the PoC opts out (the CLI's own documented opt-out) so stderr
        // stays empty and the pass criterion below stays meaningful.
        .env("ZEN_TELEMETRY", "0")
        .env("HOME", &home)
        .env_remove("ZEN_MODEL")
        .env_remove("ZEN_BASE_URL")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ZEN_SETTINGS")
        .spawn()?;
    let pid = child.id().unwrap_or(0);
    println!("run {run}: spawned pid {pid} session {session_id}");

    let mut stderr = child
        .stderr
        .take()
        .expect("stderr was piped, so it is present");
    let stderr_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });

    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped, so it is present");
    let mut lines = BufReader::new(stdout).lines();
    let mut envelopes: Vec<EnvelopeTiming> = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    let mut seq = 0usize;
    while let Some(line) = lines.next_line().await? {
        let arrival_ms = t0.elapsed().as_secs_f64() * 1000.0;
        if line.trim().is_empty() {
            continue;
        }
        seq += 1;
        match parse_stream_json(&line) {
            Ok(parsed) => {
                for cli_line in parsed {
                    let (line_type, subtype) = label_event(&cli_line.event);
                    let desktop_events = to_desktop_events(&cli_line).len();
                    println!(
                        "run {run} seq {seq} +{arrival_ms:.1}ms {line_type}/{subtype} \
                         ({} bytes, {desktop_events} desktop events)",
                        line.len()
                    );
                    envelopes.push(EnvelopeTiming {
                        seq,
                        arrival_ms,
                        line_type,
                        subtype,
                        bytes: line.len(),
                        desktop_events,
                    });
                }
            }
            Err(e) => {
                let msg = format!("seq {seq}: {e}");
                eprintln!("run {run} parse error: {msg}");
                parse_errors.push(msg);
            }
        }
    }

    let status = child.wait().await?;
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let exit_code = status.code();
    let exit = exit_code
        .map(interpret_exit_code)
        .map(|e| format!("{e:?}"))
        .unwrap_or_else(|| "no exit code (killed by signal)".to_string());
    let stderr_text = stderr_task.await.unwrap_or_default();
    let stderr_tail: String = stderr_text
        .chars()
        .rev()
        .take(4096)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    println!("run {run}: exit {exit_code:?} ({exit}) wall {wall_ms:.1}ms");
    Ok(RunTiming {
        run,
        session_id,
        exit_code,
        exit,
        wall_ms,
        first_envelope_ms: envelopes.first().map(|e| e.arrival_ms),
        result_envelope_ms: envelopes
            .iter()
            .rev()
            .find(|e| e.line_type == "result")
            .map(|e| e.arrival_ms),
        envelopes,
        stderr_tail,
        parse_errors,
    })
}

fn run_to_json(run: &RunTiming) -> Value {
    let gaps: Vec<f64> = run
        .envelopes
        .windows(2)
        .map(|w| w[1].arrival_ms - w[0].arrival_ms)
        .collect();
    json!({
        "run": run.run,
        "session_id": run.session_id,
        "exit_code": run.exit_code,
        "exit": run.exit,
        "wall_ms": run.wall_ms,
        "first_envelope_ms": run.first_envelope_ms,
        "result_envelope_ms": run.result_envelope_ms,
        "envelope_count": run.envelopes.len(),
        "envelope_types": run.envelopes.iter().map(|e| format!("{}/{}", e.line_type, e.subtype)).collect::<Vec<_>>(),
        "inter_envelope_gap_ms": stats(&gaps),
        "envelopes": run.envelopes.iter().map(|e| json!({
            "seq": e.seq,
            "arrival_ms": e.arrival_ms,
            "type": e.line_type,
            "subtype": e.subtype,
            "bytes": e.bytes,
            "desktop_events": e.desktop_events,
        })).collect::<Vec<_>>(),
        "stderr_tail": run.stderr_tail,
        "parse_errors": run.parse_errors,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let binary: PathBuf = match std::env::args().nth(1) {
        Some(b) => PathBuf::from(b),
        None => resolve_zen_binary()?,
    };
    if !binary.is_file() {
        anyhow::bail!("CLI binary not found: {}", binary.display());
    }
    let binary = binary.canonicalize()?;
    let model = env_or("POC_MODEL", "mock-model");
    let base_url = env_or("POC_BASE_URL", "http://127.0.0.1:18123/v1");
    let prompt = env_or("POC_PROMPT", "hello");
    let runs: usize = env_or("POC_RUNS", "5").parse()?;
    let max_turns: usize = env_or("POC_MAX_TURNS", "3").parse()?;
    let out_path = env_or("POC_OUT", "stdio-bridge-poc-run.json");
    let cli_rev = env_or("POC_CLI_REV", "unknown");
    let scratch = std::env::temp_dir().join(format!("stdio-bridge-poc-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;

    let mut opts = RunOptions::new(&base_url, &model, &scratch);
    opts.max_turns = max_turns;
    opts.system_prompt = String::new(); // minimal argv: the DV-3 contract form
    opts.api_key = None; // the key is set on the child env directly in run_once

    println!("stdio bridge PoC");
    println!("  binary: {}", binary.display());
    println!("  cli_rev: {cli_rev}");
    println!("  model: {model}  base_url: {base_url}  prompt: {prompt:?}  runs: {runs}");

    let mut run_timings = Vec::new();
    for run in 1..=runs {
        let timing = run_once(run, &binary, &opts, &prompt, &scratch).await?;
        run_timings.push(timing);
    }

    let walls: Vec<f64> = run_timings.iter().map(|r| r.wall_ms).collect();
    let firsts: Vec<f64> = run_timings
        .iter()
        .filter_map(|r| r.first_envelope_ms)
        .collect();
    let results: Vec<f64> = run_timings
        .iter()
        .filter_map(|r| r.result_envelope_ms)
        .collect();
    let all_gaps: Vec<f64> = run_timings
        .iter()
        .flat_map(|r| {
            r.envelopes
                .windows(2)
                .map(|w| w[1].arrival_ms - w[0].arrival_ms)
                .collect::<Vec<_>>()
        })
        .collect();

    let report = json!({
        "tool": "stdio_bridge_poc",
        "contract": "stream-json v1 (docs/stream-json-v1.md)",
        "transport": "stdio NDJSON: child process stdout, one JSON object per line",
        "binary": binary.display().to_string(),
        "cli_rev": cli_rev,
        "model": model,
        "base_url": base_url,
        "prompt": prompt,
        "max_turns": max_turns,
        "runs": run_timings.iter().map(run_to_json).collect::<Vec<_>>(),
        "summary": {
            "wall_ms": stats(&walls),
            "time_to_first_envelope_ms": stats(&firsts),
            "time_to_result_envelope_ms": stats(&results),
            "inter_envelope_gap_ms": stats(&all_gaps),
        },
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&report)?)?;
    println!("report written: {out_path}");
    println!(
        "summary wall_ms: {}",
        serde_json::to_string(&report["summary"]["wall_ms"])?
    );
    println!(
        "summary time_to_first_envelope_ms: {}",
        serde_json::to_string(&report["summary"]["time_to_first_envelope_ms"])?
    );
    println!(
        "summary time_to_result_envelope_ms: {}",
        serde_json::to_string(&report["summary"]["time_to_result_envelope_ms"])?
    );

    // The PoC passes only when every run is a real, clean, complete run:
    // exit 0, no parse errors, a result line present, stderr empty.
    let mut ok = true;
    for r in &run_timings {
        if r.exit_code != Some(0)
            || r.exit != format!("{:?}", CliExit::Success)
            || !r.parse_errors.is_empty()
            || r.result_envelope_ms.is_none()
            || !r.stderr_tail.trim().is_empty()
        {
            eprintln!(
                "run {} FAILED: exit {:?} ({}) parse_errors={} result={:?} stderr={:?}",
                r.run,
                r.exit_code,
                r.exit,
                r.parse_errors.len(),
                r.result_envelope_ms,
                r.stderr_tail
            );
            ok = false;
        }
        if r.envelopes.is_empty()
            || r.envelopes.first().map(|e| e.line_type.as_str()) != Some("system")
        {
            eprintln!("run {} FAILED: first envelope is not system/init", r.run);
            ok = false;
        }
    }
    if !ok {
        anyhow::bail!("one or more PoC runs failed (see above)");
    }
    println!("STDIO BRIDGE POC OK");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_empty_is_null() {
        let s = stats(&[]);
        assert_eq!(s["n"], 0);
        assert!(s["min_ms"].is_null());
    }

    #[test]
    fn stats_odd_and_even_median() {
        assert_eq!(median_sorted(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(median_sorted(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
    }

    #[test]
    fn percentile_nearest_rank() {
        // 5 samples: p95 is the max (nearest-rank), p50 is the 3rd value.
        let v = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile_sorted(&v, 95.0), Some(50.0));
        assert_eq!(percentile_sorted(&v, 50.0), Some(30.0));
        assert_eq!(percentile_sorted(&v, 1.0), Some(10.0));
    }

    #[test]
    fn label_event_covers_all_variants() {
        let (t, s) = label_event(&CliStreamEvent::Result {
            subtype: "success".into(),
            is_error: false,
            result: None,
            session_id: None,
            total_cost_usd: None,
            duration_ms: None,
            num_turns: None,
            usage: Default::default(),
            model: None,
        });
        assert_eq!((t, s), ("result".to_string(), "success".to_string()));
    }
}
