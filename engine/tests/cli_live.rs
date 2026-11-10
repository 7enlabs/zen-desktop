//! Step-8 live test: the REAL CLI binary (03eb057) against the mock LLM
//! stub, driven through the Desktop's own `run_cli_task` (Unix only).
//! `#[ignore]`d — needs a locally built CLI binary + the stub; never runs
//! in CI or in a normal `cargo test` pass.
//!
//! Run on the box (stub is started/stopped by the test itself):
//!   DV3_LIVE_ZEN_BIN=/data/home/muse2/zen \
//!   DV3_LIVE_STUB=/data/home/muse2/mock_llm_stub.py \
//!   cargo test --offline --locked --test cli_live -- --ignored --test-threads=1
//!
//! The test never touches the network: the stub binds 127.0.0.1 only and
//! the CLI points at it via --base-url. The mock model is "mock-model:free"
//! (the free-only policy rejects a bare "mock-model"); the stub ignores the
//! requested model and always answers as "mock-model".

#![cfg(unix)]

#[path = "common/fake_zen.rs"]
mod fake_zen;

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use zencode_engine::budget::TaskBudget;
use zencode_engine::cli_spawn::{run_cli_task_with_binary, CliRunHandle};
use zencode_engine::events::{DesktopEvent, RecordingSink};
use zencode_engine::scope::ScopeDecl;
use zencode_engine::stream_json::CliExit;
use zencode_engine::RunOptions;

fn zen_bin() -> PathBuf {
    let p = std::env::var("DV3_LIVE_ZEN_BIN")
        .expect("DV3_LIVE_ZEN_BIN must point at the real CLI binary (03eb057)");
    let p = PathBuf::from(p);
    assert!(
        p.is_file(),
        "DV3_LIVE_ZEN_BIN is not a file: {}",
        p.display()
    );
    p
}

fn stub_path() -> PathBuf {
    let p = std::env::var("DV3_LIVE_STUB").expect("DV3_LIVE_STUB must point at mock_llm_stub.py");
    let p = PathBuf::from(p);
    assert!(p.is_file(), "DV3_LIVE_STUB is not a file: {}", p.display());
    p
}

/// A running stub; killed on drop so a failed assert never leaks a server.
struct Stub {
    child: Child,
}

impl Stub {
    fn start(mode: &str, write_path: Option<&Path>, delay_ms: u32, port: u16) -> Stub {
        let mut cmd = Command::new("python3");
        cmd.arg(stub_path())
            .env("MOCK_LLM_MODE", mode)
            .env("MOCK_LLM_PORT", port.to_string())
            .env("MOCK_LLM_DELAY_MS", delay_ms.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(p) = write_path {
            cmd.env("MOCK_LLM_WRITE_PATH", p);
        }
        let mut child = cmd.spawn().expect("start mock LLM stub");
        // Wait for the port to accept connections (stub is up).
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("mock LLM stub did not start on port {port} within 10s");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Stub { child }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn opts_for(dir: &Path, port: u16) -> RunOptions {
    let mut opts = RunOptions::new(
        format!("http://127.0.0.1:{port}/v1"),
        "mock-model:free",
        dir.to_path_buf(),
    );
    opts.api_key = Some("test-key".to_string());
    opts.max_turns = 3;
    opts.scope = Some(ScopeDecl {
        files: vec![],
        dirs: vec![dir.to_path_buf()],
        description: "live test".to_string(),
    });
    opts
}

fn has_permission_denied(sink: &RecordingSink) -> bool {
    sink.events()
        .iter()
        .any(|e| matches!(e.event, DesktopEvent::PermissionDenied { .. }))
}

fn session_end_count(sink: &RecordingSink) -> usize {
    sink.events()
        .iter()
        .filter(|e| matches!(e.event, DesktopEvent::SessionEnd { .. }))
        .count()
}

#[tokio::test]
#[ignore]
async fn live_cli_against_mock() {
    // Isolate the CLI from the operator's settings, like the tier-3 runbook.
    let home = fake_zen::tmpdir("live-home");
    std::env::set_var("HOME", &home);
    std::env::set_var("ZEN_TRUST_FOLDER", "1");
    for v in [
        "ZEN_MODEL",
        "ZEN_BASE_URL",
        "OPENAI_API_KEY",
        "ZEN_SETTINGS",
    ] {
        std::env::remove_var(v);
    }
    let bin = zen_bin();
    let dir = fake_zen::tmpdir("live");

    // Phase 1 — happy path (text reply): init, assistant, result.
    {
        let _stub = Stub::start("text", None, 0, 18123);
        let sink = Arc::new(RecordingSink::new());
        let handle = CliRunHandle::new();
        let outcome = run_cli_task_with_binary(
            &opts_for(&dir, 18123),
            "say hello",
            sink.clone(),
            &handle,
            Some(bin.clone()),
        )
        .await
        .unwrap();
        assert_eq!(outcome.exit, CliExit::Success);
        assert!(outcome.success, "{}", outcome.result);
        assert_eq!(outcome.result, "Hello from the mock LLM.");
        assert_eq!(outcome.num_turns, 1);
        assert_eq!(outcome.input_tokens, 7);
        assert_eq!(outcome.output_tokens, 3);
        // The outcome carries the model the Desktop asked for; the CLI
        // echoes that same name in its own init/result lines (verified
        // live: --model mock-model:free -> result model "mock-model:free").
        assert_eq!(outcome.model, "mock-model:free");
        assert_eq!(session_end_count(&sink), 1, "SessionEnd exactly once");
        let served = sink
            .events()
            .iter()
            .find_map(|e| match &e.event {
                DesktopEvent::SessionEnd { model, .. } => Some(model.clone()),
                _ => None,
            })
            .expect("SessionEnd present");
        assert_eq!(served, "mock-model:free");
        assert!(!has_permission_denied(&sink));
    }

    // Phase 2 — dangerous tool, default permission mode: the CLI must NOT
    // run Write (decision 4, tin 9105).
    let target_a = dir.join("proof-a.txt");
    {
        let _stub = Stub::start("write_tool", Some(&target_a), 0, 18124);
        let sink = Arc::new(RecordingSink::new());
        let handle = CliRunHandle::new();
        let mut opts = opts_for(&dir, 18124);
        opts.dangerous_allow_all = false;
        let outcome = run_cli_task_with_binary(
            &opts,
            "write the word hello to proof-a.txt",
            sink.clone(),
            &handle,
            Some(bin.clone()),
        )
        .await
        .unwrap();
        assert!(outcome.success, "{}", outcome.result);
        assert!(
            has_permission_denied(&sink),
            "expected a PermissionDenied event for Write; log:\n{}",
            sink.to_ndjson().unwrap()
        );
        assert!(
            !target_a.exists(),
            "the CLI must not create proof-a.txt in default permission mode"
        );
    }

    // Phase 3 — same prompt with bypassPermissions: the tool runs.
    let target_b = dir.join("proof-b.txt");
    {
        let _stub = Stub::start("write_tool", Some(&target_b), 0, 18125);
        let sink = Arc::new(RecordingSink::new());
        let handle = CliRunHandle::new();
        let mut opts = opts_for(&dir, 18125);
        opts.dangerous_allow_all = true;
        let outcome = run_cli_task_with_binary(
            &opts,
            "write the word hello to proof-b.txt",
            sink.clone(),
            &handle,
            Some(bin.clone()),
        )
        .await
        .unwrap();
        assert!(outcome.success, "{}", outcome.result);
        assert!(
            !has_permission_denied(&sink),
            "no denial expected with bypassPermissions"
        );
        assert!(target_b.exists(), "the CLI must create proof-b.txt");
        assert_eq!(
            std::fs::read_to_string(&target_b).unwrap(),
            "written by mock"
        );
    }

    // Phase 4 — user stop while the CLI waits on a slow LLM reply.
    {
        let _stub = Stub::start("text", None, 30_000, 18126);
        let sink = Arc::new(RecordingSink::new());
        let handle = CliRunHandle::new();
        let handle2 = handle.clone();
        let bin4 = bin.clone();
        let opts4 = opts_for(&dir, 18126);
        let run = tokio::spawn(async move {
            run_cli_task_with_binary(&opts4, "say hello", sink, &handle2, Some(bin4)).await
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.request_stop("user stop");
        let outcome = tokio::time::timeout(Duration::from_secs(20), run)
            .await
            .expect("run must finish after stop")
            .unwrap()
            .unwrap();
        assert_eq!(outcome.exit, CliExit::Interrupted, "SIGINT exit is 130");
        assert_eq!(outcome.stop_reason.as_deref(), Some("user stop"));
        assert!(!outcome.success);
    }

    // Phase 5 — timeout while the CLI waits on a slow LLM reply.
    {
        let _stub = Stub::start("text", None, 30_000, 18127);
        let sink = Arc::new(RecordingSink::new());
        let mut opts = opts_for(&dir, 18127);
        opts.budget = Some(TaskBudget {
            max_duration_secs: 1,
            ..TaskBudget::unlimited()
        });
        let handle = CliRunHandle::new();
        let outcome = run_cli_task_with_binary(&opts, "say hello", sink, &handle, Some(bin))
            .await
            .unwrap();
        assert!(outcome.timed_out);
        assert_eq!(outcome.stop_reason.as_deref(), Some("timeout"));
        assert!(outcome.result.contains("timed out"), "{}", outcome.result);
    }

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
}
