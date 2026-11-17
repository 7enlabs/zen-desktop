use std::collections::VecDeque;
use std::path::Path;
use zen_core::*;

struct Scripted {
    replies: VecDeque<Result<Reply, Error>>,
    seen: Vec<Vec<Message>>,
}

impl Scripted {
    fn new(replies: Vec<Result<Reply, Error>>) -> Self {
        Scripted {
            replies: replies.into(),
            seen: Vec::new(),
        }
    }
}

impl Llm for Scripted {
    fn complete(&mut self, context: &[Message], _: &[ToolSpec]) -> Result<Reply, Error> {
        self.seen.push(context.to_vec());
        self.replies
            .pop_front()
            .unwrap_or_else(|| Err(Error::Llm("script exhausted".into())))
    }
}

struct Echo;
impl Tool for Echo {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "echo input".into(),
        }
    }
    fn call(&mut self, input: &str, sb: &dyn Sandbox) -> Result<String, Error> {
        if !sb.allow_exec("echo") {
            return Err(Error::Denied("exec echo".into()));
        }
        Ok(input.to_string())
    }
}

struct AllowEcho;
impl Sandbox for AllowEcho {
    fn isolation(&self) -> Isolation {
        Isolation::Partial("test policy, no OS enforcement".into())
    }
    fn allow_read(&self, _: &Path) -> bool {
        false
    }
    fn allow_write(&self, _: &Path) -> bool {
        false
    }
    fn allow_exec(&self, p: &str) -> bool {
        p == "echo"
    }
}

fn call(name: &str, input: &str) -> Result<Reply, Error> {
    Ok(Reply::Call(ToolCall {
        name: name.into(),
        input: input.into(),
    }))
}

fn tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(Echo)]
}

#[test]
fn one_full_loop_with_tool_call() {
    let mut llm = Scripted::new(vec![call("echo", "hi"), Ok(Reply::Text("done".into()))]);
    let mut log = MemoryLog::new();
    let out = run_turn(&mut llm, &mut tools(), &AllowEcho, &mut log, "say hi", 4).unwrap();
    assert_eq!(
        out,
        TurnOutcome {
            text: "done".into(),
            steps: 2
        }
    );
    let ctx = log.replay_context();
    let roles: Vec<Role> = ctx.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::User, Role::Tool, Role::Assistant]);
    assert_eq!(ctx[1].content, "hi");
}

#[test]
fn everything_model_sees_is_logged() {
    let mut llm = Scripted::new(vec![call("echo", "x"), Ok(Reply::Text("ok".into()))]);
    let mut log = MemoryLog::new();
    run_turn(&mut llm, &mut tools(), &AllowEcho, &mut log, "go", 4).unwrap();
    let logged = log.replay_context();
    for ctx in &llm.seen {
        assert_eq!(&logged[..ctx.len()], &ctx[..], "model saw unlogged context");
    }
}

#[test]
fn sandbox_denial_is_fed_back_not_hidden() {
    let mut llm = Scripted::new(vec![call("echo", "x"), Ok(Reply::Text("ok".into()))]);
    let mut log = MemoryLog::new();
    run_turn(&mut llm, &mut tools(), &NoSandbox, &mut log, "go", 4).unwrap();
    let tool_msg = &log.replay_context()[1];
    assert_eq!(tool_msg.role, Role::Tool);
    assert!(
        tool_msg.content.starts_with("ERROR: sandbox denied"),
        "{}",
        tool_msg.content
    );
    assert_eq!(NoSandbox.isolation(), Isolation::None);
}

#[test]
fn unknown_tool_and_step_limit_are_reported() {
    let mut llm = Scripted::new(vec![call("nope", ""), call("nope", "")]);
    let mut log = MemoryLog::new();
    let err = run_turn(&mut llm, &mut tools(), &AllowEcho, &mut log, "go", 2).unwrap_err();
    assert_eq!(err, Error::StepLimit(2));
    assert!(log.replay_context()[1]
        .content
        .contains("unknown tool `nope`"));
    assert!(matches!(log.events().last(), Some(Event::Error(_))));
}

#[test]
fn llm_error_is_logged_and_returned() {
    let mut llm = Scripted::new(vec![Err(Error::Llm("rate limited".into()))]);
    let mut log = MemoryLog::new();
    let err = run_turn(&mut llm, &mut tools(), &AllowEcho, &mut log, "go", 3).unwrap_err();
    assert_eq!(err, Error::Llm("rate limited".into()));
    assert_eq!(
        log.events().last(),
        Some(&Event::Error("llm error: rate limited".into()))
    );
}

#[test]
fn second_turn_resumes_from_log_and_fork_branches() {
    let mut log = MemoryLog::new();
    let mut llm = Scripted::new(vec![Ok(Reply::Text("a1".into()))]);
    run_turn(&mut llm, &mut tools(), &AllowEcho, &mut log, "q1", 2).unwrap();
    let branch_point = log.events().len();

    let mut llm2 = Scripted::new(vec![Ok(Reply::Text("a2".into()))]);
    run_turn(&mut llm2, &mut tools(), &AllowEcho, &mut log, "q2", 2).unwrap();

    assert_eq!(llm2.seen[0].len(), 3);
    assert_eq!(llm2.seen[0][0].content, "q1");

    let mut fork = log.fork(branch_point);
    let mut llm3 = Scripted::new(vec![Ok(Reply::Text("b2".into()))]);
    run_turn(
        &mut llm3,
        &mut tools(),
        &AllowEcho,
        &mut fork,
        "other q2",
        2,
    )
    .unwrap();
    let texts: Vec<String> = fork
        .replay_context()
        .into_iter()
        .map(|m| m.content)
        .collect();
    assert_eq!(texts, ["q1", "a1", "other q2", "b2"]);
    assert_eq!(log.replay_context().len(), 4);
    assert_eq!(log.fork(999).events().len(), log.events().len());
}
