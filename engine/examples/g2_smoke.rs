use std::sync::Arc;

use zencode_engine::cli_spawn::{run_cli_task, CliRunHandle};
use zencode_engine::events::RecordingSink;
use zencode_engine::RunOptions;

const BASE_URL: &str = "https://ai-gateway.kilo.ai/api/v1";
const MODEL: &str = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join("zencode-g2-smoke");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;

    let mut opts = RunOptions::new(BASE_URL, MODEL, &dir);
    opts.api_key = None;
    opts.max_turns = 8;
    opts.dangerous_allow_all = true;

    let sink = Arc::new(RecordingSink::new());
    let task = "Create a file named hello.txt in the working directory containing exactly this text on one line: hello zencode";
    println!("task: {task}");

    let handle = CliRunHandle::new();
    let outcome = run_cli_task(&opts, task, sink.clone(), &handle).await?;
    println!("run outcome: {outcome:?}");
    assert!(outcome.success, "smoke run must succeed");

    let content = std::fs::read_to_string(dir.join("hello.txt"))?;
    println!("hello.txt: {content:?}");
    assert_eq!(content.trim(), "hello zencode", "file content mismatch");

    let events = sink.events();
    println!("events recorded: {}", events.len());
    println!("G2 SMOKE OK");
    Ok(())
}
