//! Fake `zen` scripts for cli_spawn integration tests (Unix only).
//!
//! Each helper writes a small `sh` script whose behaviour is baked into
//! the script text, so tests never mutate the process environment (parallel
//! tests share it). The scripts are test-only scaffolding, not a mock LLM:
//! they print canned stream-json lines or sleep, nothing more.

//! Shared fake-`zen` helpers for the subprocess integration tests.
//! Each test binary compiles this file separately and uses only some
//! helpers, so dead-code lints are silenced here on purpose.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use zencode_engine::scope::ScopeDecl;
use zencode_engine::RunOptions;

/// Sequence number making each temp script name unique within a process.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Sequence number making each tmpdir unique within a process.
static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn tmpdir(tag: &str) -> PathBuf {
    // A fresh directory per call: never remove a path another test (or a
    // still-running child) may be using, and never reuse a path an earlier
    // call in this process used. The tag keeps the directory readable in
    // failure output; pid + sequence make it unique.
    let seq = DIR_SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("zen_fake_{tag}_{}_{seq}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    // Write to a temp file in the same directory, then rename it into
    // place, so the destination path is never observed half-written:
    // exec'ing a script while it is still open for writing fails with
    // ETXTBSY ("Text file busy") on Linux, which flaked these tests.
    // The temp name is unique per call so parallel tests cannot collide.
    let seq = WRITE_SEQ.fetch_add(1, Ordering::SeqCst);
    let tmp_path = dir.join(format!(".{name}.tmp-{}-{seq}", std::process::id()));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp_path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::rename(&tmp_path, &path).unwrap();
    path
}

/// A fake zen that prints the given NDJSON lines to stdout and exits 0.
pub fn stream_script(dir: &Path, lines: &str) -> PathBuf {
    let lines_path = dir.join("lines.ndjson");
    std::fs::write(&lines_path, lines).unwrap();
    write_script(
        dir,
        "zen",
        &format!("#!/bin/sh\ncat \"{}\"\n", lines_path.display()),
    )
}

/// A fake zen that never prints anything and sleeps (dies on SIGINT, exit 130).
pub fn sleep_script(dir: &Path) -> PathBuf {
    write_script(dir, "zen", "#!/bin/sh\nexec sleep 60\n")
}

/// A fake zen that prints text to stderr and exits with the given code.
pub fn fail_script(dir: &Path, stderr: &str, code: i32) -> PathBuf {
    write_script(
        dir,
        "zen",
        &format!("#!/bin/sh\necho '{stderr}' >&2\nexit {code}\n"),
    )
}

/// A fake zen that fails the first call with a 429 on stderr, then streams
/// the given NDJSON lines on later calls (fallback-retry test).
pub fn counting_script(dir: &Path, lines: &str) -> PathBuf {
    let lines_path = dir.join("lines.ndjson");
    let state_path = dir.join("state");
    std::fs::write(&lines_path, lines).unwrap();
    write_script(
        dir,
        "zen",
        &format!(
            "#!/bin/sh\nn=$(cat \"{}\" 2>/dev/null || echo 0)\nn=$((n + 1))\necho \"$n\" > \"{}\"\nif [ \"$n\" -eq 1 ]; then echo \"HTTP 429: rate limit exceeded\" >&2; exit 1; fi\ncat \"{}\"\n",
            state_path.display(),
            state_path.display(),
            lines_path.display()
        ),
    )
}

/// Minimal well-formed stream: init, one assistant text, one success result.
pub fn ok_stream() -> String {
    [
        r#"{"v":1,"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000001","tools":["Read"],"model":"mock-model:free"}"#,
        r#"{"v":1,"type":"assistant","message":{"role":"assistant","content":"hello"}}"#,
        r#"{"v":1,"type":"result","subtype":"success","is_error":false,"result":"hello","session_id":"00000000-0000-4000-8000-000000000001","num_turns":1,"usage":{"input_tokens":7,"output_tokens":3},"model":"mock-model:free"}"#,
    ]
    .join("\n")
}

pub fn base_opts(dir: &Path) -> RunOptions {
    let mut opts = RunOptions::new(
        "http://127.0.0.1:18123/v1",
        "mock-model:free",
        dir.to_path_buf(),
    );
    opts.scope = Some(ScopeDecl {
        files: vec![],
        dirs: vec![dir.to_path_buf()],
        description: "test".to_string(),
    });
    opts
}
