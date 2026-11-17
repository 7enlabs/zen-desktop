//! DV-4 tier 3 — live-CLI check (IGNORED by default).
//!
//! Parses a REAL binary's stream-json output captured from a subprocess run
//! (see docs/dv4-tier3-runbook.md) with the real Desktop parser. This test
//! never runs in a normal `cargo test` pass: it is `#[ignore]`d and reads its
//! input from the DV4_TIER3_CAPTURE env var. Run it explicitly after a
//! tier-3 capture:
//!   DV4_TIER3_CAPTURE=~/muse2/tier3-output.jsonl cargo test --offline --locked --test tier3_live_cli -- --ignored
//!
//! Invariants (same as tiers 1-2): the parser is never modified to force
//! green; a failing assert is reported as a finding, not weakened.

use zen_engine::events::DesktopEvent;
use zen_engine::stream_json::{interpret_exit_code, parse_stream_json, to_desktop_events, CliExit};

#[test]
#[ignore]
fn tier3_live_cli_output_parses_and_projects() {
    let path = std::env::var("DV4_TIER3_CAPTURE")
        .expect("DV4_TIER3_CAPTURE must point at a captured tier-3 output file");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let lines = parse_stream_json(&text).expect("live CLI output must parse as stream-json v1");
    assert_eq!(
        lines.len(),
        3,
        "live run: expected system + assistant + result"
    );
    assert!(
        lines.iter().all(|l| l.v == 1),
        "every live line must carry v = 1"
    );
    let types: Vec<&str> = lines.iter().map(|l| l.line_type.as_str()).collect();
    assert_eq!(types, ["system", "assistant", "result"], "live line order");
    for line in &lines {
        assert!(
            !to_desktop_events(line).is_empty(),
            "line {} must project to >= 1 event",
            line.line_type
        );
    }
    match &to_desktop_events(&lines[0])[..] {
        [DesktopEvent::SystemMessage { event, .. }] => assert_eq!(event, "cli_system:init"),
        other => panic!("system line must project to SystemMessage, got {other:?}"),
    }
    match &to_desktop_events(&lines[1])[..] {
        [DesktopEvent::AssistantMessage { text, .. }] => {
            assert_eq!(text.as_deref(), Some("Hello from the mock LLM."));
        }
        other => panic!("assistant line must project to AssistantMessage, got {other:?}"),
    }
    match &to_desktop_events(&lines[2])[..] {
        [DesktopEvent::SessionEnd {
            success,
            num_turns,
            model,
            input_tokens,
            output_tokens,
            ..
        }] => {
            assert!(*success, "live result must be success");
            assert_eq!(*num_turns, 1);
            assert_eq!(model, "mock-model");
            assert_eq!(*input_tokens, 7);
            assert_eq!(*output_tokens, 3);
        }
        other => panic!("result line must project to SessionEnd, got {other:?}"),
    }
    assert_eq!(interpret_exit_code(0), CliExit::Success);
}
