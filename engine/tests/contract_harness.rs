//! DV-4 contract-test harness — tier 1 (runnable on the night of 30/10/2026).
//!
//! Tier 1 checks the HARD contract requirements at fixture level (raw JSON) using
//! `serde_json` — independent of the parser, because the parser `engine/src/stream_json.rs`
//! was not yet on main `aeaf42d` (it lived on the DV-2 line, see the design doc).
//!
//! Tier 2 (pinning the real parser) is fully specified in
//! `docs/dv4-contract-harness-design.md` and could only be activated after PR #3
//! (DV-2) merged: at that point `zencode_engine::stream_json::{parse_stream_json,
//! to_desktop_events, interpret_exit_code}` would exist and the following tier-2 asserts
//! would be added verbatim, without changing tier-1 asserts:
//!   - unknown type -> `DesktopEvent::Unknown`, not dropped (fixture 09);
//!   - truncated stream -> `parse_stream_json` returns `Err`, i.e. 0 events (fixture 17);
//!   - physical-line numbering of errors, including blank lines (fixture 15: line 1, 17: line 3);
//!   - result -> `SessionEnd` with `success = !is_error && subtype == "success"`,
//!     model, usage, duration (fixture 06/07/12/20);
//!   - exit codes 0/1/130/143 -> Success/Error/Interrupted/Terminated.
//!
//! Run: `cargo test --offline --locked --test contract_harness`
//! Per-fixture report: `cargo test --offline --locked --test contract_harness
//! -- --nocapture` (test `fixture_inventory_report` prints one line per fixture).

use std::path::PathBuf;

use serde_json::Value;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stream_json")
        .join(name)
}

fn fixture_text(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("đọc fixture {}: {e}", path.display()))
}

/// The JSON records of a fixture: blank lines skipped, order preserved.
/// Panics with the physical line number (starting at 1) if a line is not a JSON object.
fn records(name: &str) -> Vec<Value> {
    let text = fixture_text(name);
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("fixture {name} dòng {}: JSON lỗi: {e}", i + 1));
        assert!(
            v.is_object(),
            "fixture {name} dòng {}: mỗi dòng NDJSON phải là một object",
            i + 1
        );
        out.push(v);
    }
    out
}

/// The first physical line that fails JSON parsing in a fixture, with its line number (from 1).
/// Returns `None` if every line (including blank lines) parses or is empty.
fn first_bad_line(name: &str) -> Option<(usize, String)> {
    for (i, line) in fixture_text(name).lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Err(e) = serde_json::from_str::<Value>(line) {
            return Some((i + 1, e.to_string()));
        }
    }
    None
}

fn record_type(v: &Value) -> &str {
    v.get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("record thiếu trường type: {v}"))
}

/// The valid fixtures (every line is a JSON object, v = 1).
const VALID_FIXTURES: &[&str] = &[
    "01_system_init.jsonl",
    "02_assistant_text.jsonl",
    "03_assistant_tool_use.jsonl",
    "04_user_tool_result.jsonl",
    "05_user_tool_result_error.jsonl",
    "06_result_success.jsonl",
    "07_result_error.jsonl",
    "08_full_session.jsonl",
    "09_unknown_type.jsonl",
    "11_assistant_tool_calls_no_text.jsonl",
    "12_result_with_model.jsonl",
    "18_no_trailing_newline.jsonl",
    "19_multiple_tool_calls.jsonl",
    "20_result_error_with_model.jsonl",
    "21_user_two_tool_results.jsonl",
    "22_unknown_field_on_known_type.jsonl",
];

#[test]
fn all_valid_fixtures_carry_schema_v1() {
    for name in VALID_FIXTURES {
        for (idx, rec) in records(name).iter().enumerate() {
            assert_eq!(
                rec.get("v").and_then(Value::as_u64),
                Some(1),
                "fixture {name} record {idx}: v phải bằng 1, thực tế: {rec}"
            );
        }
    }
}

#[test]
fn every_valid_record_has_a_type() {
    for name in VALID_FIXTURES {
        for rec in records(name) {
            let _ = record_type(&rec);
        }
    }
}

#[test]
fn no_valid_fixture_requires_seq() {
    // Contract v1: no `seq` field; the consumer numbers lines itself if needed.
    for name in VALID_FIXTURES {
        for rec in records(name) {
            assert!(
                rec.get("seq").is_none(),
                "fixture {name}: record mang trường seq, trái hợp đồng v1: {rec}"
            );
        }
    }
}

#[test]
fn blank_lines_yield_no_records() {
    assert!(records("13_empty_input.jsonl").is_empty());
    let blank = fixture_text("14_blank_lines_only.jsonl");
    assert_eq!(blank.lines().count(), 3, "fixture 14 phải có 3 dòng vật lý");
    assert!(records("14_blank_lines_only.jsonl").is_empty());
}

#[test]
fn system_init_carries_tools_and_model() {
    let recs = records("01_system_init.jsonl");
    assert_eq!(recs.len(), 1);
    let rec = &recs[0];
    assert_eq!(record_type(rec), "system");
    assert_eq!(rec.get("subtype").and_then(Value::as_str), Some("init"));
    assert_eq!(
        rec.get("session_id").and_then(Value::as_str),
        Some("ses_abc123")
    );
    assert_eq!(
        rec.get("model").and_then(Value::as_str),
        Some("cohere/north-mini-code:free")
    );
    let tools = rec
        .get("tools")
        .and_then(Value::as_array)
        .expect("system/init phải có mảng tools");
    let names: Vec<&str> = tools.iter().map(|t| t.as_str().unwrap()).collect();
    assert_eq!(names, ["read", "write", "bash"]);
}

#[test]
fn assistant_text_shape() {
    let recs = records("02_assistant_text.jsonl");
    assert_eq!(recs.len(), 1);
    let blocks = recs[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("02: message.content phải là mảng block");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].get("type").and_then(Value::as_str), Some("text"));
    assert_eq!(
        blocks[0].get("text").and_then(Value::as_str),
        Some("Tôi sẽ đọc file cấu hình trước.")
    );
}

#[test]
fn assistant_tool_calls_are_openai_shape() {
    // HARD shape: message.tool_calls, each call {id, type: "function",
    // function: {name, arguments}} with arguments a JSON-encoded string.
    let recs = records("03_assistant_tool_use.jsonl");
    let msg = recs[0].get("message").expect("03: thiếu message");
    assert_eq!(
        msg.get("content").and_then(Value::as_str),
        Some("Đọc file:")
    );
    let calls = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .expect("03: thiếu message.tool_calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("id").and_then(Value::as_str),
        Some("call_read_01")
    );
    assert_eq!(
        calls[0].get("type").and_then(Value::as_str),
        Some("function")
    );
    let fun = calls[0].get("function").expect("03: thiếu function");
    assert_eq!(fun.get("name").and_then(Value::as_str), Some("read"));
    let args = fun
        .get("arguments")
        .and_then(Value::as_str)
        .expect("03: arguments phải là chuỗi JSON-encoded");
    let parsed: Value = serde_json::from_str(args).expect("03: arguments phải parse được");
    assert_eq!(
        parsed.get("path").and_then(Value::as_str),
        Some("tauri.conf.json")
    );
}

#[test]
fn assistant_tool_calls_without_text_omit_content() {
    // When a message only calls tools, content is omitted (no null sent).
    let recs = records("11_assistant_tool_calls_no_text.jsonl");
    let msg = recs[0].get("message").expect("11: thiếu message");
    assert!(
        msg.get("content").is_none(),
        "11: content phải bị lược, không phải null"
    );
    let calls = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .expect("11: thiếu message.tool_calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("id").and_then(Value::as_str),
        Some("call_task_1")
    );
    assert_eq!(
        calls[0]
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str),
        Some("Task")
    );
}

#[test]
fn assistant_multiple_tool_calls_keep_order() {
    let recs = records("19_multiple_tool_calls.jsonl");
    let calls = recs[0]
        .get("message")
        .and_then(|m| m.get("tool_calls"))
        .and_then(Value::as_array)
        .expect("19: thiếu message.tool_calls");
    assert_eq!(calls.len(), 2);
    let ids: Vec<&str> = calls
        .iter()
        .map(|c| c.get("id").and_then(Value::as_str).unwrap())
        .collect();
    let names: Vec<&str> = calls
        .iter()
        .map(|c| {
            c.get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap()
        })
        .collect();
    assert_eq!(ids, ["call_read_02", "call_bash_01"]);
    assert_eq!(names, ["read", "bash"]);
}

#[test]
fn user_tool_results_shape_and_order() {
    let one = records("04_user_tool_result.jsonl");
    let blocks = one[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("04: thiếu content blocks");
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].get("type").and_then(Value::as_str),
        Some("tool_result")
    );
    assert_eq!(
        blocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_01")
    );
    // Absent is_error means no error (default false at the parser layer).
    assert!(blocks[0].get("is_error").is_none());

    let err = records("05_user_tool_result_error.jsonl");
    let eblocks = err[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("05: thiếu content blocks");
    assert_eq!(
        eblocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_02")
    );
    assert_eq!(
        eblocks[0].get("is_error").and_then(Value::as_bool),
        Some(true)
    );

    let two = records("21_user_two_tool_results.jsonl");
    let tblocks = two[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .expect("21: thiếu content blocks");
    assert_eq!(tblocks.len(), 2);
    assert_eq!(
        tblocks[0].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_11")
    );
    assert_eq!(
        tblocks[1].get("tool_use_id").and_then(Value::as_str),
        Some("toolu_12")
    );
    assert_eq!(
        tblocks[1].get("is_error").and_then(Value::as_bool),
        Some(true)
    );
}

/// Inputs that the parser layer (tier 2) must project into SessionEnd:
/// success is derived from `!is_error && subtype == "success"`.
fn session_end_inputs(rec: &Value) -> (bool, &str, &str) {
    let subtype = rec.get("subtype").and_then(Value::as_str).unwrap();
    let is_error = rec.get("is_error").and_then(Value::as_bool).unwrap();
    let model = rec.get("model").and_then(Value::as_str).unwrap_or("");
    (!is_error && subtype == "success", subtype, model)
}

#[test]
fn result_lines_carry_session_end_inputs() {
    let ok = records("06_result_success.jsonl");
    assert_eq!(ok.len(), 1);
    let (success, subtype, model) = session_end_inputs(&ok[0]);
    assert!(success);
    assert_eq!(subtype, "success");
    assert_eq!(ok[0].get("num_turns").and_then(Value::as_u64), Some(3));
    assert_eq!(
        ok[0]
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_u64),
        Some(1200)
    );
    assert_eq!(
        ok[0]
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_u64),
        Some(340)
    );
    assert_eq!(
        ok[0].get("duration_ms").and_then(Value::as_u64),
        Some(45210)
    );
    // Fixture 06 carries no model: the parser layer defaults to an empty model (Level B).
    assert_eq!(model, "");

    let err = records("07_result_error.jsonl");
    let (success, _, _) = session_end_inputs(&err[0]);
    assert!(!success);
    assert!(err[0]
        .get("result")
        .and_then(Value::as_str)
        .unwrap()
        .contains("429"));

    let with_model = records("12_result_with_model.jsonl");
    let (success, _, model) = session_end_inputs(&with_model[0]);
    assert!(success);
    assert_eq!(model, "cohere/north-mini-code:free");

    let err_model = records("20_result_error_with_model.jsonl");
    let (success, _, model) = session_end_inputs(&err_model[0]);
    assert!(!success);
    assert_eq!(model, "cohere/north-mini-code:free");
    assert!(err_model[0]
        .get("result")
        .and_then(Value::as_str)
        .unwrap()
        .contains("429"));
}

#[test]
fn unknown_type_record_is_preserved_in_stream() {
    // HARD requirement at stream level: an unknown record is not dropped from the stream.
    // (Projecting to DesktopEvent::Unknown is a tier-2 assert, after the DV-2 merge.)
    let recs = records("09_unknown_type.jsonl");
    assert_eq!(recs.len(), 1);
    assert_eq!(record_type(&recs[0]), "heartbeat");
    assert!(recs[0].get("at").is_some(), "09: trường lạ phải còn nguyên");
}

#[test]
fn unknown_fields_on_known_types_are_tolerated() {
    // Additive change: an unknown field on a known record must not
    // break the stream. Fixture 22 is the only new fixture of tonight's harness.
    let recs = records("22_unknown_field_on_known_type.jsonl");
    assert_eq!(recs.len(), 2);
    assert_eq!(record_type(&recs[0]), "system");
    assert!(recs[0].get("future_field").is_some());
    assert_eq!(
        recs[0].get("another_unknown").and_then(Value::as_u64),
        Some(123)
    );
    assert_eq!(record_type(&recs[1]), "result");
    assert_eq!(
        recs[1].get("future_result_field").and_then(Value::as_str),
        Some("ignore-me")
    );
}

#[test]
fn bad_version_is_rejected_input() {
    let recs_text = fixture_text("10_bad_version.jsonl");
    let v: Value = serde_json::from_str(recs_text.trim()).expect("10: phải là JSON");
    assert_eq!(v.get("v").and_then(Value::as_u64), Some(2));
    // v != 1, so a v1 consumer must reject the whole stream (tier-2 assert).
}

#[test]
fn missing_version_is_rejected_input() {
    let v: Value = serde_json::from_str(fixture_text("16_missing_version.jsonl").trim())
        .expect("16: phải là JSON");
    assert!(v.get("v").is_none(), "16: record thiếu trường v");
}

#[test]
fn malformed_json_fails_on_physical_line_1() {
    let (line, _) = first_bad_line("15_malformed_json.jsonl").expect("15: phải có dòng JSON lỗi");
    assert_eq!(line, 1);
}

#[test]
fn truncated_stream_fails_on_physical_line_3() {
    // Fail-fast input condition: first 2 lines valid, line 3 truncated.
    // Fail-fast behaviour (return Err, 0 events) is a tier-2 assert, after the DV-2 merge.
    let text = fixture_text("17_truncated_stream.jsonl");
    let mut good = 0;
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(_) => good += 1,
            Err(_) => {
                assert_eq!(i + 1, 3, "17: dòng đứt phải là dòng vật lý 3");
                assert_eq!(good, 2, "17: 2 dòng trước dòng đứt phải hợp lệ");
                return;
            }
        }
    }
    panic!("17: không tìm thấy dòng đứt");
}

#[test]
fn final_line_without_trailing_newline_is_a_complete_record() {
    let text = fixture_text("18_no_trailing_newline.jsonl");
    assert!(
        !text.ends_with(char::from(10)),
        "18: file khong duoc ket thuc bang newline"
    );
    let recs = records("18_no_trailing_newline.jsonl");
    assert_eq!(recs.len(), 1);
    assert_eq!(record_type(&recs[0]), "result");
    assert_eq!(recs[0].get("v").and_then(Value::as_u64), Some(1));
}

#[test]
fn full_session_order_is_preserved() {
    let recs = records("08_full_session.jsonl");
    let types: Vec<&str> = recs.iter().map(record_type).collect();
    assert_eq!(
        types,
        ["system", "assistant", "assistant", "user", "result"]
    );
    let last = recs.last().unwrap();
    assert_eq!(last.get("subtype").and_then(Value::as_str), Some("success"));
    assert_eq!(last.get("is_error").and_then(Value::as_bool), Some(false));
}

#[test]
fn fixture_inventory_report() {
    // Per-fixture report: run with `-- --nocapture` to see this table.
    // Each line: fixture name, record count (blank lines excluded), types in order.
    // Rejected fixtures (10/15/16/17) are reported separately, not counted in the record table.
    let mut names: Vec<&str> = VALID_FIXTURES.to_vec();
    names.push("13_empty_input.jsonl");
    names.push("14_blank_lines_only.jsonl");
    for name in names {
        let recs = records(name);
        let types: Vec<&str> = recs.iter().map(record_type).collect();
        println!("fixture={name} records={} types={:?}", recs.len(), types);
    }
    // 10 and 16 are valid JSON but rejected at the version layer (v=2 / missing v),
    // not at the JSON parse layer — report the reason clearly instead of an error line number.
    println!("fixture=10_bad_version.jsonl rejected_input reason=unsupported_version v=2");
    let (line15, _) = first_bad_line("15_malformed_json.jsonl").expect("15: phải có dòng JSON lỗi");
    println!("fixture=15_malformed_json.jsonl rejected_input bad_line={line15}");
    println!("fixture=16_missing_version.jsonl rejected_input reason=missing_version");
    let (line, _) = first_bad_line("17_truncated_stream.jsonl").expect("17: phải có dòng đứt");
    println!("fixture=17_truncated_stream.jsonl rejected_input bad_line={line}");
}

// Tier 2 (DV-4): pin the REAL parser against the same fixtures.
// Added after PR #3 merged (main 4b71b23): zencode_engine::stream_json
// now exists on main. Tier-1 tests above are untouched; tier 2 only adds
// assertions that call the parser. Invariant: never edit the parser to
// force green; a failing tier-2 assert is reported as a finding.

use zencode_engine::events::DesktopEvent;
use zencode_engine::stream_json::{
    interpret_exit_code, parse_stream_json, to_desktop_events, CliExit, CliLine, CliStreamEvent,
};

fn tier2_parsed(name: &str) -> Vec<CliLine> {
    let text = fixture_text(name);
    parse_stream_json(&text).unwrap_or_else(|e| panic!("parse fixture {name}: {e}"))
}

fn tier2_types(lines: &[CliLine]) -> Vec<&str> {
    lines.iter().map(|l| l.line_type.as_str()).collect()
}

#[test]
fn tier2_valid_fixtures_parse_counts_and_types() {
    // (fixture, expected line count, expected line_type order)
    let cases: &[(&str, usize, &[&str])] = &[
        ("01_system_init.jsonl", 1, &["system"]),
        ("02_assistant_text.jsonl", 1, &["assistant"]),
        ("03_assistant_tool_use.jsonl", 1, &["assistant"]),
        ("04_user_tool_result.jsonl", 1, &["user"]),
        ("05_user_tool_result_error.jsonl", 1, &["user"]),
        ("06_result_success.jsonl", 1, &["result"]),
        ("07_result_error.jsonl", 1, &["result"]),
        (
            "08_full_session.jsonl",
            5,
            &["system", "assistant", "assistant", "user", "result"],
        ),
        ("11_assistant_tool_calls_no_text.jsonl", 1, &["assistant"]),
        ("12_result_with_model.jsonl", 1, &["result"]),
        ("18_no_trailing_newline.jsonl", 1, &["result"]),
        ("19_multiple_tool_calls.jsonl", 1, &["assistant"]),
        ("20_result_error_with_model.jsonl", 1, &["result"]),
        ("21_user_two_tool_results.jsonl", 1, &["user"]),
        (
            "22_unknown_field_on_known_type.jsonl",
            2,
            &["system", "result"],
        ),
    ];
    for (name, count, types) in cases {
        let lines = tier2_parsed(name);
        assert_eq!(lines.len(), *count, "fixture {name}: line count");
        assert!(lines.iter().all(|l| l.v == 1), "fixture {name}: v == 1");
        assert_eq!(tier2_types(&lines), *types, "fixture {name}: line types");
    }
}

#[test]
fn tier2_unknown_type_is_surfaced_not_dropped() {
    let lines = tier2_parsed("09_unknown_type.jsonl");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_type, "heartbeat");
    assert_eq!(
        to_desktop_events(&lines[0]),
        vec![DesktopEvent::Unknown {
            variant: "cli_stream:heartbeat".to_string()
        }]
    );
}

#[test]
fn tier2_truncated_stream_fails_fast() {
    let err = parse_stream_json(&fixture_text("17_truncated_stream.jsonl"))
        .expect_err("17: truncated stream must fail");
    let msg = err.to_string();
    assert!(msg.contains("line 3"), "17: error mentions line 3: {msg}");
    assert!(
        msg.contains("invalid JSON"),
        "17: error mentions invalid JSON: {msg}"
    );
}

#[test]
fn tier2_malformed_json_fails_on_line_1() {
    let err = parse_stream_json(&fixture_text("15_malformed_json.jsonl"))
        .expect_err("15: malformed JSON must fail");
    let msg = err.to_string();
    assert!(msg.contains("line 1"), "15: error mentions line 1: {msg}");
    assert!(
        msg.contains("invalid JSON"),
        "15: error mentions invalid JSON: {msg}"
    );
}

#[test]
fn tier2_bad_and_missing_version_rejected() {
    for name in ["10_bad_version.jsonl", "16_missing_version.jsonl"] {
        let err = parse_stream_json(&fixture_text(name)).expect_err(&format!("{name}: must fail"));
        assert!(
            err.to_string().contains("unsupported schema version"),
            "{name}: error mentions unsupported schema version: {err}"
        );
    }
}

#[test]
fn tier2_empty_and_blank_only_parse_to_empty() {
    for name in ["13_empty_input.jsonl", "14_blank_lines_only.jsonl"] {
        let lines = parse_stream_json(&fixture_text(name))
            .unwrap_or_else(|e| panic!("{name}: must parse Ok: {e}"));
        assert!(lines.is_empty(), "{name}: expected empty vec");
    }
}

fn tier2_session_end(lines: &[CliLine]) -> DesktopEvent {
    let evs = to_desktop_events(&lines[0]);
    assert_eq!(evs.len(), 1, "result line must map to exactly 1 event");
    evs.into_iter().next().unwrap()
}

#[test]
fn tier2_result_maps_to_session_end() {
    // 06: full SessionEnd values.
    let lines = tier2_parsed("06_result_success.jsonl");
    match tier2_session_end(&lines) {
        DesktopEvent::SessionEnd {
            success,
            num_turns,
            input_tokens,
            output_tokens,
            duration_ms,
            model,
            ..
        } => {
            assert!(success);
            assert_eq!(num_turns, 3);
            assert_eq!(input_tokens, 1200);
            assert_eq!(output_tokens, 340);
            assert_eq!(duration_ms, 45210);
            assert_eq!(model, "");
        }
        other => panic!("06: expected SessionEnd, got {other:?}"),
    }
    // 07: error result, result text carries 429.
    let lines = tier2_parsed("07_result_error.jsonl");
    match tier2_session_end(&lines) {
        DesktopEvent::SessionEnd {
            success, result, ..
        } => {
            assert!(!success);
            assert!(result.contains("429"), "07: result mentions 429: {result}");
        }
        other => panic!("07: expected SessionEnd, got {other:?}"),
    }
    // 12: model pinned.
    let lines = tier2_parsed("12_result_with_model.jsonl");
    match tier2_session_end(&lines) {
        DesktopEvent::SessionEnd { model, .. } => {
            assert_eq!(model, "cohere/north-mini-code:free");
        }
        other => panic!("12: expected SessionEnd, got {other:?}"),
    }
    // 20: error result with model.
    let lines = tier2_parsed("20_result_error_with_model.jsonl");
    match tier2_session_end(&lines) {
        DesktopEvent::SessionEnd { success, model, .. } => {
            assert!(!success);
            assert_eq!(model, "cohere/north-mini-code:free");
        }
        other => panic!("20: expected SessionEnd, got {other:?}"),
    }
}

#[test]
fn tier2_system_init_maps_to_system_message() {
    let lines = tier2_parsed("01_system_init.jsonl");
    let evs = to_desktop_events(&lines[0]);
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        DesktopEvent::SystemMessage { event, text } => {
            assert_eq!(event, "cli_system:init");
            assert!(
                text.contains("ses_abc123"),
                "01: text carries session: {text}"
            );
            assert!(
                text.contains("cohere/north-mini-code:free"),
                "01: text carries model: {text}"
            );
        }
        other => panic!("01: expected SystemMessage, got {other:?}"),
    }
}

#[test]
fn tier2_assistant_maps_to_assistant_message() {
    // 03: text + one tool call.
    let lines = tier2_parsed("03_assistant_tool_use.jsonl");
    let evs = to_desktop_events(&lines[0]);
    match &evs[0] {
        DesktopEvent::AssistantMessage {
            text, tool_calls, ..
        } => {
            assert_eq!(text.as_deref(), Some("Đọc file:"));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_read_01");
            assert_eq!(tool_calls[0].name, "read");
        }
        other => panic!("03: expected AssistantMessage, got {other:?}"),
    }
    // 19: two tool calls in order.
    let lines = tier2_parsed("19_multiple_tool_calls.jsonl");
    let evs = to_desktop_events(&lines[0]);
    match &evs[0] {
        DesktopEvent::AssistantMessage { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 2);
            assert_eq!(tool_calls[0].id, "call_read_02");
            assert_eq!(tool_calls[0].name, "read");
            assert_eq!(tool_calls[1].id, "call_bash_01");
            assert_eq!(tool_calls[1].name, "bash");
        }
        other => panic!("19: expected AssistantMessage, got {other:?}"),
    }
    // 11: tool-only message, no text.
    let lines = tier2_parsed("11_assistant_tool_calls_no_text.jsonl");
    let evs = to_desktop_events(&lines[0]);
    match &evs[0] {
        DesktopEvent::AssistantMessage {
            text, tool_calls, ..
        } => {
            assert_eq!(text, &None);
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_task_1");
            assert_eq!(tool_calls[0].name, "Task");
        }
        other => panic!("11: expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn tier2_user_maps_to_tool_results() {
    let lines = tier2_parsed("21_user_two_tool_results.jsonl");
    let evs = to_desktop_events(&lines[0]);
    assert_eq!(evs.len(), 2);
    match &evs[0] {
        DesktopEvent::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_11");
            assert!(!is_error);
        }
        other => panic!("21 first: expected ToolResult, got {other:?}"),
    }
    match &evs[1] {
        DesktopEvent::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_12");
            assert!(is_error);
        }
        other => panic!("21 second: expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tier2_exit_codes() {
    assert_eq!(interpret_exit_code(0), CliExit::Success);
    assert_eq!(interpret_exit_code(1), CliExit::Error);
    assert_eq!(interpret_exit_code(130), CliExit::Interrupted);
    assert_eq!(interpret_exit_code(143), CliExit::Terminated);
    assert_eq!(interpret_exit_code(2), CliExit::Other(2));
}

#[test]
fn tier2_additive_fields_do_not_change_result() {
    let lines = tier2_parsed("22_unknown_field_on_known_type.jsonl");
    assert_eq!(lines.len(), 2);
    match &lines[0].event {
        CliStreamEvent::System(sys) => assert_eq!(sys.subtype, "init"),
        other => panic!("22 first: expected System, got {other:?}"),
    }
    match &lines[1].event {
        CliStreamEvent::Result {
            subtype, is_error, ..
        } => {
            assert_eq!(subtype, "success");
            assert!(!is_error);
        }
        other => panic!("22 last: expected Result, got {other:?}"),
    }
}

#[test]
fn tier2_projection_never_drops_a_line() {
    // Every parsed line of every valid fixture maps to >= 1 DesktopEvent,
    // including unknown types (spec: "surface, never silently change").
    let names = [
        "01_system_init.jsonl",
        "02_assistant_text.jsonl",
        "03_assistant_tool_use.jsonl",
        "04_user_tool_result.jsonl",
        "05_user_tool_result_error.jsonl",
        "06_result_success.jsonl",
        "07_result_error.jsonl",
        "08_full_session.jsonl",
        "11_assistant_tool_calls_no_text.jsonl",
        "12_result_with_model.jsonl",
        "18_no_trailing_newline.jsonl",
        "19_multiple_tool_calls.jsonl",
        "20_result_error_with_model.jsonl",
        "21_user_two_tool_results.jsonl",
        "22_unknown_field_on_known_type.jsonl",
    ];
    for name in names {
        let lines = tier2_parsed(name);
        for line in &lines {
            let evs = to_desktop_events(line);
            assert!(
                !evs.is_empty(),
                "{name} line {} maps to 0 events",
                line.line_type
            );
        }
    }
}
