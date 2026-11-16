use std::io::Write;
use zen_core::*;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&d);
    d
}

fn sample() -> Vec<Event> {
    vec![
        Event::Context(Message::new(
            Role::User,
            "xin chào \"Zen\"\n\ttab \\ ✓ 😀 \u{1}",
        )),
        Event::Call(ToolCall {
            name: "shell".into(),
            input: "{\"cmd\":\"ls\"}".into(),
        }),
        Event::Context(Message::new(Role::Tool, "ok")),
        Event::Error("boom".into()),
    ]
}

#[test]
fn roundtrip_write_read_replay_equal() {
    let p = tmp("rt");
    let mut a = JsonlLog::open(&p).unwrap();
    for e in sample() {
        a.append(e).unwrap();
    }
    let b = JsonlLog::open(&p).unwrap();
    assert_eq!(a.events(), b.events());
    assert_eq!(a.replay_context(), b.replay_context());
    assert_eq!(b.events().len(), 4);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn torn_last_line_keeps_earlier_and_appends_cleanly() {
    let p = tmp("torn");
    let mut a = JsonlLog::open(&p).unwrap();
    for e in sample() {
        a.append(e).unwrap();
    }
    drop(a);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&p)
        .unwrap()
        .write_all(b"{\"t\":\"ctx\",\"role\":\"us")
        .unwrap();
    let mut b = JsonlLog::open(&p).unwrap();
    assert_eq!(b.events(), &sample()[..]);
    b.append(Event::Error("after".into())).unwrap();
    let c = JsonlLog::open(&p).unwrap();
    assert_eq!(c.events().len(), 5);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn corrupt_middle_line_is_an_error() {
    let p = tmp("mid");
    std::fs::write(&p, "garbage\n{\"t\":\"err\",\"msg\":\"x\"}\n").unwrap();
    assert!(matches!(JsonlLog::open(&p), Err(Error::Log(_))));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn dedupe_same_content_once() {
    let out = dedupe_instructions(vec![
        ("AGENTS.md".into(), "rules\n".into()),
        ("CLAUDE.md".into(), "rules".into()),
        ("OTHER.md".into(), "different".into()),
    ]);
    let names: Vec<_> = out.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(names, ["AGENTS.md", "OTHER.md"]);
}
