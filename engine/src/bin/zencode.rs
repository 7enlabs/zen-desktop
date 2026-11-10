use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use zencode_engine::cli_spawn::{run_cli_task, CliRunHandle};
use zencode_engine::events::RecordingSink;
use zencode_engine::scope::ScopeDecl;
use zencode_engine::RunOptions;

const DEFAULT_BASE_URL: &str = "https://ai-gateway.kilo.ai/api/v1";
const DEFAULT_MODEL: &str = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free";

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!(
        "  zencode run \"<task>\" [--model M] [--cwd DIR] [--max-turns N] [--session-dir DIR]"
    );
    eprintln!("  zencode eval [--model M] [--max-turns N]");
    std::process::exit(2);
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn base_opts(model: &str, cwd: PathBuf, max_turns: usize) -> RunOptions {
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let mut opts = RunOptions::new(env_or("ZENCODE_BASE_URL", DEFAULT_BASE_URL), model, cwd);
    opts.api_key = std::env::var("ZENCODE_API_KEY").ok();
    opts.max_turns = max_turns;

    opts.scope = Some(ScopeDecl {
        files: vec![],
        dirs: vec![opts.cwd.clone()],
        description: "current working directory".to_owned(),
    });
    opts
}

fn check_free(model: &str) -> anyhow::Result<()> {
    if !model.ends_with(":free") {
        anyhow::bail!("refusing non-free model '{model}': only `:free` models are allowed");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    match cmd.as_str() {
        "run" => cmd_run(args).await,
        "eval" => cmd_eval(args).await,
        _ => usage(),
    }
}

async fn cmd_run(mut args: std::iter::Skip<std::env::Args>) -> anyhow::Result<()> {
    let task = args.next().unwrap_or_default();
    if task.is_empty() || task.starts_with("--") {
        usage();
    }

    let mut model = env_or("ZENCODE_MODEL", DEFAULT_MODEL);
    let mut cwd = std::env::current_dir()?;
    let mut max_turns = 12usize;
    let mut session_dir: Option<PathBuf> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().unwrap_or_else(|| usage()),
            "--cwd" => cwd = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--max-turns" => {
                max_turns = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--session-dir" => {
                session_dir = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            _ => usage(),
        }
    }

    check_free(&model)?;
    let opts = base_opts(&model, cwd.clone(), max_turns);

    let sink = Arc::new(RecordingSink::new());
    let handle = CliRunHandle::new();
    let outcome = run_cli_task(&opts, &task, sink.clone(), &handle).await?;

    let dir = session_dir.unwrap_or_else(|| cwd.join(".zencode").join("sessions"));
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let path = dir.join(format!("session-{ts}.jsonl"));
    std::fs::write(&path, sink.to_ndjson()?)?;

    println!("result: {}", outcome.result);
    println!("turns: {}", outcome.num_turns);
    println!(
        "tokens: {} in / {} out",
        outcome.input_tokens, outcome.output_tokens
    );
    println!("cost_usd: {}", outcome.total_cost_usd);
    println!("session: {}", path.display());
    if !outcome.success {
        std::process::exit(1);
    }
    Ok(())
}

struct EvalTask {
    name: &'static str,
    setup: &'static [(&'static str, &'static str)],
    prompt: &'static str,

    expect: &'static [(&'static str, &'static str)],
}

const EVAL_TASKS: &[EvalTask] = &[
    EvalTask {
        name: "create-file",
        setup: &[],
        prompt: "Create a file named a.txt containing exactly: alpha",
        expect: &[("a.txt", "alpha")],
    },
    EvalTask {
        name: "create-nested",
        setup: &[],
        prompt: "Create sub/b.txt containing exactly: beta",
        expect: &[("sub/b.txt", "beta")],
    },
    EvalTask {
        name: "read-then-edit",
        setup: &[("e.txt", "hello world")],
        prompt: "In e.txt, replace the word 'world' with 'zen' (read it first if needed)",
        expect: &[("e.txt", "hello zen")],
    },
    EvalTask {
        name: "overwrite",
        setup: &[("w.txt", "old content")],
        prompt: "Replace the entire content of w.txt with exactly: new content",
        expect: &[("w.txt", "new content")],
    },
    EvalTask {
        name: "two-files",
        setup: &[],
        prompt: "Create x.txt containing exactly 'ex' and y.txt containing exactly 'why'",
        expect: &[("x.txt", "ex"), ("y.txt", "why")],
    },
    EvalTask {
        name: "fix-typo",
        setup: &[("t.txt", "teh quick brown fox")],
        prompt: "Fix the typo in t.txt: 'teh' should be 'the'",
        expect: &[("t.txt", "the quick brown fox")],
    },
    EvalTask {
        name: "append-line",
        setup: &[("l.txt", "line1")],
        prompt: "Append a second line 'line2' to l.txt",
        expect: &[("l.txt", "line1\nline2")],
    },
    EvalTask {
        name: "bash-rename",
        setup: &[("old.txt", "rename me")],
        prompt: "Using a shell command, rename old.txt to new.txt",
        expect: &[("new.txt", "rename me")],
    },
    EvalTask {
        name: "read-report",
        setup: &[("r.txt", "the secret is 42")],
        prompt: "Read r.txt and tell me what it says (do not modify anything)",
        expect: &[("r.txt", "the secret is 42")],
    },
    EvalTask {
        name: "mkdir-and-write",
        setup: &[],
        prompt:
            "Create a directory named docs and write notes.txt inside it containing exactly: hi",
        expect: &[("docs/notes.txt", "hi")],
    },
];

async fn cmd_eval(mut args: std::iter::Skip<std::env::Args>) -> anyhow::Result<()> {
    let mut model = env_or("ZENCODE_MODEL", DEFAULT_MODEL);
    let mut max_turns = 12usize;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().unwrap_or_else(|| usage()),
            "--max-turns" => {
                max_turns = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            _ => usage(),
        }
    }
    check_free(&model)?;

    println!(
        "{:<16} {:>6} {:>6} {:>9} {:>9} {:>6}",
        "task", "turns", "pass", "tok_in", "tok_out", "429s"
    );
    let mut passed = 0;
    let mut total_in = 0u64;
    let mut total_out = 0u64;
    let mut total_turns = 0u64;

    for t in EVAL_TASKS {
        let dir = std::env::temp_dir().join(format!(
            "zencode-eval-{}-{}",
            t.name,
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        ));
        std::fs::create_dir_all(&dir)?;
        for (rel, content) in t.setup {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, content)?;
        }

        let opts = base_opts(&model, dir.clone(), max_turns);
        let sink = Arc::new(RecordingSink::new());
        let handle = CliRunHandle::new();
        let res = run_cli_task(&opts, t.prompt, sink, &handle).await;

        let mut ok = false;
        let (turns, tok_in, tok_out) = match &res {
            Ok(r) => {
                ok = r.success
                    && t.expect.iter().all(|(rel, want)| {
                        std::fs::read_to_string(dir.join(rel))
                            .map(|got| got.trim() == *want)
                            .unwrap_or(false)
                    });

                if ok && t.name == "bash-rename" {
                    ok = !dir.join("old.txt").exists();
                }
                (r.num_turns, r.input_tokens, r.output_tokens)
            }
            Err(_) => (0, 0, 0),
        };
        if ok {
            passed += 1;
        }
        total_turns += turns;
        total_in += tok_in;
        total_out += tok_out;

        println!(
            "{:<16} {:>6} {:>6} {:>9} {:>9} {:>6}",
            t.name,
            turns,
            if ok { "PASS" } else { "FAIL" },
            tok_in,
            tok_out,
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    println!(
        "----\n{}/{} passed, {} turns, {} in / {} out tokens",
        passed,
        EVAL_TASKS.len(),
        total_turns,
        total_in,
        total_out
    );
    Ok(())
}
