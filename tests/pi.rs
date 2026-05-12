use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use millracer::command::{CommandExecutor, CommandResult, ProcessHandle};
use millracer::pi::{PiConfig, PiHarness, discover_default_skill_paths};
use millracer::pi_rpc::{
    PiRpcHarness, PiRpcSession, PiSessionFactory, PiSessionLike, RpcChild, RpcStreamEvent,
    build_rpc_command,
};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedRun {
    args: Vec<String>,
    cwd: PathBuf,
    timeout: Option<Duration>,
}

#[derive(Debug)]
struct RecordingExecutor {
    calls: RefCell<Vec<RecordedRun>>,
    stdout: String,
    stderr: String,
    returncode: i32,
}

impl RecordingExecutor {
    fn success(stdout: &str) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            stdout: stdout.to_owned(),
            stderr: String::new(),
            returncode: 0,
        }
    }
}

impl CommandExecutor for RecordingExecutor {
    fn run(
        &self,
        args: &[String],
        cwd: &Path,
        timeout: Option<Duration>,
        _env: Option<&BTreeMap<String, String>>,
    ) -> millracer::MillracerResult<CommandResult> {
        self.calls.borrow_mut().push(RecordedRun {
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
            timeout,
        });
        Ok(CommandResult {
            args: args.to_vec(),
            returncode: self.returncode,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        })
    }

    fn start(
        &self,
        _args: &[String],
        _cwd: &Path,
        _env: Option<&BTreeMap<String, String>>,
    ) -> millracer::MillracerResult<Box<dyn ProcessHandle>> {
        Err(millracer::MillracerError::message(
            "start is not used by these tests",
        ))
    }
}

#[test]
fn pi_harness_injects_prompt_skills_thinking_and_print_mode() {
    let executor = RecordingExecutor::success("ok\n\n");
    let harness = PiHarness {
        config: PiConfig {
            command: "pi".to_owned(),
            provider: Some("openai".to_owned()),
            model: Some("gpt-5.4-mini".to_owned()),
            thinking: Some("high".to_owned()),
            skill_paths: vec![PathBuf::from("/skills/delegate")],
            extra_args: vec!["--extra".to_owned()],
        },
        executor,
    };

    let output = harness
        .complete(
            "hello",
            Path::new("/tmp/repo"),
            Some(Duration::from_secs(7)),
        )
        .expect("pi completion");

    assert_eq!(output, "ok");
    let calls = harness.executor.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].cwd, PathBuf::from("/tmp/repo"));
    assert_eq!(calls[0].timeout, Some(Duration::from_secs(7)));
    let args = &calls[0].args;
    assert_eq!(args[0], "pi");
    assert_eq!(args[1], "--print");
    assert!(args.contains(&"--no-context-files".to_owned()));
    assert!(args.contains(&"--append-system-prompt".to_owned()));
    assert!(
        args[args
            .iter()
            .position(|arg| arg == "--append-system-prompt")
            .expect("system prompt flag")
            + 1]
        .contains("Do not batch independent queue items")
    );
    assert_flag_value(args, "--provider", "openai");
    assert_flag_value(args, "--model", "gpt-5.4-mini");
    assert_flag_value(args, "--thinking", "high");
    assert_flag_value(args, "--skill", "/skills/delegate");
    assert!(args.iter().any(|arg| arg == "--extra"));
    assert_eq!(args.last().map(String::as_str), Some("hello"));
}

#[test]
fn pi_harness_surfaces_nonzero_command_output() {
    let harness = PiHarness {
        config: PiConfig::default(),
        executor: RecordingExecutor {
            calls: RefCell::new(Vec::new()),
            stdout: String::new(),
            stderr: "bad pi".to_owned(),
            returncode: 9,
        },
    };

    let error = harness
        .complete("hello", Path::new("/tmp/repo"), None)
        .expect_err("nonzero pi command should fail");

    assert!(error.to_string().contains("command failed (9): pi --print"));
    assert!(error.to_string().contains("bad pi"));
}

#[test]
fn default_skill_discovery_finds_standard_dev_layout() {
    let root =
        std::env::temp_dir().join(format!("millracer-skill-discovery-{}", std::process::id()));
    let skills = root.join("source/millrace/docs/skills");
    let delegation = skills.join("millrace-autonomous-delegation");
    let operator = skills.join("millrace-ops-agent-manual");
    std::fs::create_dir_all(&delegation).expect("delegation skill dir");
    std::fs::create_dir_all(&operator).expect("operator skill dir");

    let discovered = discover_default_skill_paths(Some(&root.join("workspace/src/main.rs")));

    assert_eq!(discovered, vec![delegation, operator]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rpc_command_uses_persistent_mode_without_print() {
    let command = build_rpc_command(&PiConfig {
        command: "pi".to_owned(),
        provider: Some("openai".to_owned()),
        model: Some("gpt-5.4-mini".to_owned()),
        thinking: Some("high".to_owned()),
        skill_paths: vec![PathBuf::from("/skills/delegate")],
        extra_args: vec!["--extra".to_owned()],
    });

    assert_eq!(command[..4], ["pi", "--mode", "rpc", "--no-session"]);
    assert!(!command.iter().any(|arg| arg == "--print"));
    assert!(command.contains(&"--no-context-files".to_owned()));
    assert_flag_value(&command, "--provider", "openai");
    assert_flag_value(&command, "--model", "gpt-5.4-mini");
    assert_flag_value(&command, "--thinking", "high");
    assert_flag_value(&command, "--skill", "/skills/delegate");
    assert!(command.iter().any(|arg| arg == "--extra"));
}

#[test]
fn rpc_session_speaks_prompt_jsonl_and_returns_last_assistant_text() {
    let state = Rc::new(RefCell::new(FakeChildState::default()));
    let child = FakeRpcChild::new(
        Rc::clone(&state),
        vec![
            Some(RpcStreamEvent::Line(
                r#"{"type":"response","id":"prompt-1","success":true}"#.to_owned(),
            )),
            Some(RpcStreamEvent::Line(
                r#"{"type":"agent_message","role":"assistant"}"#.to_owned(),
            )),
            Some(RpcStreamEvent::Line(r#"{"type":"agent_end"}"#.to_owned())),
            Some(RpcStreamEvent::Line(
                r#"{"type":"response","id":"last-assistant-2","success":true,"data":{"text":"final answer"}}"#
                    .to_owned(),
            )),
        ],
    );
    let mut session = PiRpcSession::from_child(
        vec!["pi".to_owned(), "--mode".to_owned(), "rpc".to_owned()],
        Path::new("/tmp/repo"),
        BTreeMap::new(),
        Box::new(child),
    );

    let output = session
        .prompt("do work", Some(Duration::from_secs(10)))
        .expect("rpc prompt");

    assert_eq!(output, "final answer");
    let writes = parse_writes(&state.borrow().writes);
    assert_eq!(writes[0]["id"], "prompt-1");
    assert_eq!(writes[0]["type"], "prompt");
    assert_eq!(writes[0]["message"], "do work");
    assert_eq!(writes[1]["id"], "last-assistant-2");
    assert_eq!(writes[1]["type"], "get_last_assistant_text");
}

#[test]
fn rpc_session_aborts_and_closes_on_response_timeout() {
    let state = Rc::new(RefCell::new(FakeChildState::default()));
    let child = FakeRpcChild::new(Rc::clone(&state), vec![None]);
    let mut session = PiRpcSession::from_child(
        vec!["pi".to_owned()],
        Path::new("/tmp/repo"),
        BTreeMap::new(),
        Box::new(child),
    );

    let error = session
        .prompt("do work", Some(Duration::from_millis(1)))
        .expect_err("timeout should fail");

    assert!(
        error
            .to_string()
            .contains("timed out waiting for pi rpc response prompt-1")
    );
    let writes = parse_writes(&state.borrow().writes);
    assert_eq!(writes[0]["type"], "prompt");
    assert_eq!(writes[1]["type"], "abort");
    let state = state.borrow();
    assert!(state.closed_stdin);
    assert_eq!(state.terminated, 1);
    assert_eq!(state.killed, 1);
}

#[test]
fn rpc_session_reports_eof_with_stderr_detail() {
    let state = Rc::new(RefCell::new(FakeChildState {
        stderr: "bad stderr\n".to_owned(),
        ..FakeChildState::default()
    }));
    let child = FakeRpcChild::new(Rc::clone(&state), vec![Some(RpcStreamEvent::Eof)]);
    let mut session = PiRpcSession::from_child(
        vec!["pi".to_owned()],
        Path::new("/tmp/repo"),
        BTreeMap::new(),
        Box::new(child),
    );

    let error = session
        .prompt("do work", Some(Duration::from_secs(1)))
        .expect_err("eof should fail");

    assert!(
        error
            .to_string()
            .contains("pi rpc stream ended: bad stderr")
    );
}

#[test]
fn rpc_harness_reuses_session_for_same_cwd_and_rejects_different_cwd() {
    let state = Rc::new(RefCell::new(FakeHarnessState::default()));
    let factory = FakeSessionFactory {
        state: Rc::clone(&state),
    };
    let mut harness = PiRpcHarness::with_session_factory(PiConfig::default(), Box::new(factory));

    assert_eq!(
        harness
            .complete("one", Path::new("/tmp/repo"), None)
            .expect("first prompt"),
        "reply-1"
    );
    assert_eq!(
        harness
            .complete("two", Path::new("/tmp/repo"), None)
            .expect("second prompt"),
        "reply-2"
    );
    let error = harness
        .complete("three", Path::new("/tmp/other-repo"), None)
        .expect_err("different cwd should fail");
    harness.close().expect("close harness");

    assert!(error.to_string().contains("persistent Pi session is bound"));
    let state = state.borrow();
    assert_eq!(state.created_cwds, vec![PathBuf::from("/tmp/repo")]);
    assert_eq!(state.prompts, vec!["one", "two"]);
    assert_eq!(state.closed, 1);
}

fn assert_flag_value(args: &[String], flag: &str, expected: &str) {
    let index = args
        .iter()
        .position(|arg| arg == flag)
        .unwrap_or_else(|| panic!("missing {flag}"));
    assert_eq!(args.get(index + 1).map(String::as_str), Some(expected));
}

fn parse_writes(writes: &[String]) -> Vec<Value> {
    writes
        .iter()
        .map(|write| serde_json::from_str(write.trim()).expect("json write"))
        .collect()
}

#[derive(Debug, Default)]
struct FakeChildState {
    writes: Vec<String>,
    stderr: String,
    closed_stdin: bool,
    terminated: usize,
    killed: usize,
}

struct FakeRpcChild {
    state: Rc<RefCell<FakeChildState>>,
    events: VecDeque<Option<RpcStreamEvent>>,
}

impl FakeRpcChild {
    fn new(state: Rc<RefCell<FakeChildState>>, events: Vec<Option<RpcStreamEvent>>) -> Self {
        Self {
            state,
            events: events.into(),
        }
    }
}

impl RpcChild for FakeRpcChild {
    fn write_stdin_line(&mut self, line: &str) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().writes.push(line.to_owned());
        Ok(())
    }

    fn read_stdout_event(
        &mut self,
        _timeout: Duration,
    ) -> millracer::MillracerResult<Option<RpcStreamEvent>> {
        Ok(self.events.pop_front().flatten())
    }

    fn stderr_text(&self) -> String {
        self.state.borrow().stderr.clone()
    }

    fn poll(&mut self) -> millracer::MillracerResult<Option<i32>> {
        Ok(None)
    }

    fn close_stdin(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().closed_stdin = true;
        Ok(())
    }

    fn terminate(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().terminated += 1;
        Ok(())
    }

    fn kill(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().killed += 1;
        Ok(())
    }

    fn wait(&mut self, _timeout: Duration) -> millracer::MillracerResult<Option<i32>> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
struct FakeHarnessState {
    created_cwds: Vec<PathBuf>,
    prompts: Vec<String>,
    closed: usize,
}

struct FakeSessionFactory {
    state: Rc<RefCell<FakeHarnessState>>,
}

impl PiSessionFactory for FakeSessionFactory {
    fn create(
        &self,
        _command: Vec<String>,
        cwd: &Path,
        _env: &BTreeMap<String, String>,
    ) -> millracer::MillracerResult<Box<dyn PiSessionLike>> {
        self.state.borrow_mut().created_cwds.push(cwd.to_path_buf());
        Ok(Box::new(FakeSession {
            state: Rc::clone(&self.state),
        }))
    }
}

struct FakeSession {
    state: Rc<RefCell<FakeHarnessState>>,
}

impl PiSessionLike for FakeSession {
    fn prompt(
        &mut self,
        prompt: &str,
        _timeout: Option<Duration>,
    ) -> millracer::MillracerResult<String> {
        let mut state = self.state.borrow_mut();
        state.prompts.push(prompt.to_owned());
        Ok(format!("reply-{}", state.prompts.len()))
    }

    fn close(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().closed += 1;
        Ok(())
    }
}
