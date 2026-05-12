use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use millracer::command::{CommandExecutor, CommandResult, ProcessHandle};
use millracer::intake::IntakeKind;
use millracer::millrace::{
    MillraceConfig, MillraceController, render_idea_document, render_probe_document,
    render_task_document,
};
use millracer::scope::ScopedWorkItem;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedCall {
    args: Vec<String>,
    cwd: PathBuf,
}

#[derive(Debug, Default)]
struct FakeExecutorState {
    calls: Vec<RecordedCall>,
    status_payloads: VecDeque<String>,
    handles: Vec<Rc<RefCell<FakeHandleState>>>,
}

#[derive(Debug, Clone)]
struct FakeExecutor {
    state: Rc<RefCell<FakeExecutorState>>,
}

impl FakeExecutor {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(FakeExecutorState::default())),
        }
    }

    fn push_status(&self, payload: &str) {
        self.state
            .borrow_mut()
            .status_payloads
            .push_back(payload.to_owned());
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.state
            .borrow()
            .calls
            .iter()
            .map(|call| call.args.clone())
            .collect()
    }

    fn cwds(&self) -> Vec<PathBuf> {
        self.state
            .borrow()
            .calls
            .iter()
            .map(|call| call.cwd.clone())
            .collect()
    }
}

impl CommandExecutor for FakeExecutor {
    fn run(
        &self,
        args: &[String],
        cwd: &Path,
        _timeout: Option<Duration>,
        _env: Option<&BTreeMap<String, String>>,
    ) -> millracer::MillracerResult<CommandResult> {
        self.state.borrow_mut().calls.push(RecordedCall {
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
        });
        let stdout = if args.iter().any(|arg| arg == "status") {
            self.state
                .borrow_mut()
                .status_payloads
                .pop_front()
                .unwrap_or_else(|| r#"{"workspace":"/tmp/ws"}"#.to_owned())
        } else {
            "ok".to_owned()
        };
        Ok(CommandResult {
            args: args.to_vec(),
            returncode: 0,
            stdout,
            stderr: String::new(),
        })
    }

    fn start(
        &self,
        args: &[String],
        cwd: &Path,
        _env: Option<&BTreeMap<String, String>>,
    ) -> millracer::MillracerResult<Box<dyn ProcessHandle>> {
        self.state.borrow_mut().calls.push(RecordedCall {
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
        });
        let state = Rc::new(RefCell::new(FakeHandleState::default()));
        self.state.borrow_mut().handles.push(Rc::clone(&state));
        Ok(Box::new(FakeHandle { state }))
    }
}

#[derive(Debug, Default)]
struct FakeHandleState {
    poll_result: Option<i32>,
    wait_results: VecDeque<millracer::MillracerResult<i32>>,
    waits: Vec<Option<Duration>>,
    terminated: usize,
    killed: usize,
}

struct FakeHandle {
    state: Rc<RefCell<FakeHandleState>>,
}

impl ProcessHandle for FakeHandle {
    fn poll(&mut self) -> millracer::MillracerResult<Option<i32>> {
        Ok(self.state.borrow().poll_result)
    }

    fn terminate(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().terminated += 1;
        Ok(())
    }

    fn kill(&mut self) -> millracer::MillracerResult<()> {
        self.state.borrow_mut().killed += 1;
        Ok(())
    }

    fn wait(&mut self, timeout: Option<Duration>) -> millracer::MillracerResult<i32> {
        let mut state = self.state.borrow_mut();
        state.waits.push(timeout);
        state.wait_results.pop_front().unwrap_or(Ok(0))
    }
}

#[test]
fn render_task_document_contains_required_millrace_fields() {
    let raw = render_task_document(
        "task-abc",
        "Benchmark task",
        "Do the thing",
        "2026-05-10T00:00:00+00:00",
        None,
    );

    assert!(raw.starts_with("# Benchmark task\n"));
    assert!(raw.contains("Task-ID: task-abc"));
    assert!(raw.contains("Target-Paths:"));
    assert!(raw.contains("Acceptance:"));
    assert!(raw.contains("Required-Checks:"));
    assert!(raw.contains("Risk:"));
}

#[test]
fn render_task_document_includes_scoped_work_contract() {
    let scoped = ScopedWorkItem {
        item_id: "M06".to_owned(),
        title: Some("Array API label encoder support".to_owned()),
        source_queue: Some("/queue.md".to_owned()),
        spec_path: Some("/srs/M06.md".to_owned()),
        completion_ref: Some("agent-impl-M06".to_owned()),
        constraints: vec!["Do not implement any other queue item.".to_owned()],
    };
    let raw = render_task_document(
        "task-abc",
        "Benchmark task",
        "Implement the selected item only",
        "2026-05-10T00:00:00+00:00",
        Some(&scoped),
    );

    assert!(raw.contains("Scoped-Work:"));
    assert!(raw.contains("Item-ID: M06"));
    assert!(raw.contains("Spec-Path: /srs/M06.md"));
    assert!(raw.contains("Completion-Ref: agent-impl-M06"));
    assert!(raw.contains("Do not batch independent queue items into this work item."));
}

#[test]
fn render_probe_and_idea_documents_match_intake_postures() {
    let scoped = ScopedWorkItem {
        item_id: "ITEM-123".to_owned(),
        title: None,
        source_queue: None,
        spec_path: None,
        completion_ref: None,
        constraints: Vec::new(),
    };

    let probe = render_probe_document(
        "probe-abc",
        "Investigate launch behavior",
        "Figure out how launch behavior should change.",
        "2026-05-10T00:00:00+00:00",
        Some(&scoped),
    );
    let idea = render_idea_document(
        "idea-abc",
        "Add operator dashboard",
        "Add a dashboard for queued runs.",
        "2026-05-10T00:00:00+00:00",
        Some(&scoped),
    );

    assert!(probe.contains("Probe-ID: probe-abc"));
    assert!(probe.contains("Do not implement code changes during this probe stage."));
    assert!(probe.contains("Which codebase areas are likely involved?"));
    assert!(probe.contains("Item-ID: ITEM-123"));
    assert!(idea.contains("Idea-ID: idea-abc"));
    assert!(idea.contains("Desired-Outcome: Add a dashboard for queued runs."));
    assert!(idea.contains("Planning-Intent:"));
    assert!(idea.contains("Item-ID: ITEM-123"));
}

#[test]
fn controller_dispatches_probe_idea_and_task_to_matching_queue_commands() {
    let workspace = test_workspace("queue");
    let executor = FakeExecutor::new();
    let controller = controller(executor.clone(), &workspace);

    let probe_path = controller
        .enqueue(IntakeKind::Probe, "Investigate runtime behavior", None)
        .expect("probe enqueue");
    let idea_path = controller
        .enqueue(IntakeKind::Idea, "Build an operator dashboard", None)
        .expect("idea enqueue");
    let task_path = controller
        .enqueue(IntakeKind::Task, "Fix src/millracer/monitor.py", None)
        .expect("task enqueue");

    assert!(
        probe_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("probe-")
    );
    assert!(
        idea_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("idea-")
    );
    assert!(
        task_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("task-")
    );
    assert_eq!(
        executor.calls(),
        vec![
            vec![
                "millrace".to_owned(),
                "queue".to_owned(),
                "add-probe".to_owned(),
                probe_path.display().to_string(),
                "--workspace".to_owned(),
                workspace.display().to_string(),
            ],
            vec![
                "millrace".to_owned(),
                "queue".to_owned(),
                "add-idea".to_owned(),
                idea_path.display().to_string(),
                "--workspace".to_owned(),
                workspace.display().to_string(),
            ],
            vec![
                "millrace".to_owned(),
                "queue".to_owned(),
                "add-task".to_owned(),
                task_path.display().to_string(),
                "--workspace".to_owned(),
                workspace.display().to_string(),
            ],
        ]
    );

    let task_doc = std::fs::read_to_string(task_path).expect("task document");
    assert!(task_doc.contains("Task-ID: task-"));
    assert!(task_doc.contains("Summary: Fix src/millracer/monitor.py"));
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn controller_uses_default_pi_mode_for_init_validate_and_daemon() {
    let executor = FakeExecutor::new();
    let mut controller = controller(executor.clone(), Path::new("/tmp/ws"));

    controller.initialize().expect("init");
    controller.validate().expect("validate");
    controller.start_daemon().expect("start daemon");

    assert_eq!(
        executor.calls(),
        vec![
            strings(["millrace", "init", "--workspace", "/tmp/ws"]),
            strings([
                "millrace",
                "compile",
                "validate",
                "--workspace",
                "/tmp/ws",
                "--mode",
                "default_pi",
            ]),
            strings([
                "millrace",
                "status",
                "--workspace",
                "/tmp/ws",
                "--format",
                "json",
            ]),
            strings([
                "millrace",
                "run",
                "daemon",
                "--workspace",
                "/tmp/ws",
                "--mode",
                "default_pi",
                "--monitor",
                "none",
            ]),
        ]
    );
}

#[test]
fn controller_clears_stale_state_before_starting_daemon() {
    let executor = FakeExecutor::new();
    executor.push_status(r#"{"workspace":"/tmp/ws","runtime_ownership_lock":"stale"}"#);
    let mut controller = controller(executor.clone(), Path::new("/tmp/ws"));

    controller.start_daemon().expect("start daemon");

    assert_eq!(
        executor.calls(),
        vec![
            strings([
                "millrace",
                "status",
                "--workspace",
                "/tmp/ws",
                "--format",
                "json",
            ]),
            strings([
                "millrace",
                "control",
                "clear-stale-state",
                "--workspace",
                "/tmp/ws",
            ]),
            strings([
                "millrace",
                "run",
                "daemon",
                "--workspace",
                "/tmp/ws",
                "--mode",
                "default_pi",
                "--monitor",
                "none",
            ]),
        ]
    );
}

#[test]
fn controller_stop_waits_for_daemon_before_clearing_stale_state() {
    let executor = FakeExecutor::new();
    executor.push_status(r#"{"workspace":"/tmp/ws"}"#);
    executor.push_status(r#"{"workspace":"/tmp/ws","runtime_ownership_lock":"stale"}"#);
    let mut controller = controller(executor.clone(), Path::new("/tmp/ws"));

    controller.start_daemon().expect("start daemon");
    controller.stop_daemon().expect("stop daemon");

    let handle = executor.state.borrow().handles[0].clone();
    let handle = handle.borrow();
    assert_eq!(handle.waits, vec![Some(Duration::from_secs(30))]);
    assert_eq!(handle.terminated, 0);
    assert_eq!(handle.killed, 0);
    assert_eq!(
        executor
            .calls()
            .into_iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>(),
        vec![
            strings([
                "millrace",
                "control",
                "clear-stale-state",
                "--workspace",
                "/tmp/ws",
            ]),
            strings([
                "millrace",
                "status",
                "--workspace",
                "/tmp/ws",
                "--format",
                "json",
            ]),
        ]
    );
}

#[test]
fn controller_stop_terminates_then_kills_when_daemon_wait_times_out() {
    let executor = FakeExecutor::new();
    executor.push_status(r#"{"workspace":"/tmp/ws"}"#);
    executor.push_status(r#"{"workspace":"/tmp/ws"}"#);
    let mut controller = controller(executor.clone(), Path::new("/tmp/ws"));

    controller.start_daemon().expect("start daemon");
    {
        let handle = executor.state.borrow().handles[0].clone();
        let mut handle = handle.borrow_mut();
        handle
            .wait_results
            .push_back(Err(millracer::MillracerError::message("timeout 30")));
        handle
            .wait_results
            .push_back(Err(millracer::MillracerError::message("timeout 5")));
        handle.wait_results.push_back(Ok(0));
    }
    controller.stop_daemon().expect("stop daemon");

    let handle = executor.state.borrow().handles[0].clone();
    let handle = handle.borrow();
    assert_eq!(
        handle.waits,
        vec![
            Some(Duration::from_secs(30)),
            Some(Duration::from_secs(5)),
            Some(Duration::from_secs(5)),
        ]
    );
    assert_eq!(handle.terminated, 1);
    assert_eq!(handle.killed, 1);
}

#[test]
fn status_loads_json_object_and_rejects_non_object() {
    let executor = FakeExecutor::new();
    executor.push_status(r#"{"workspace":"/tmp/ws"}"#);
    executor.push_status(r#"[]"#);
    let controller = controller(executor, Path::new("/tmp/ws"));

    assert_eq!(
        controller
            .status()
            .expect("status")
            .get("workspace")
            .and_then(serde_json::Value::as_str),
        Some("/tmp/ws")
    );
    let error = controller.status().expect_err("non-object status");
    assert!(error.to_string().contains("status JSON must be an object"));
}

#[test]
fn controller_uses_cwd_override_for_subprocesses() {
    let executor = FakeExecutor::new();
    let controller = MillraceController::with_executor(
        MillraceConfig {
            command: "millrace".to_owned(),
            mode: "custom_mode".to_owned(),
        },
        executor.clone(),
        PathBuf::from("/tmp/ws"),
        Some(PathBuf::from("/tmp/project")),
    );

    controller.validate().expect("validate");

    assert_eq!(executor.cwds(), vec![PathBuf::from("/tmp/project")]);
    assert_eq!(
        executor.calls(),
        vec![strings([
            "millrace",
            "compile",
            "validate",
            "--workspace",
            "/tmp/ws",
            "--mode",
            "custom_mode",
        ])]
    );
}

fn controller(executor: FakeExecutor, workspace: &Path) -> MillraceController<FakeExecutor> {
    MillraceController::with_executor(
        MillraceConfig {
            command: "millrace".to_owned(),
            mode: "default_pi".to_owned(),
        },
        executor,
        workspace.to_path_buf(),
        None,
    )
}

fn test_workspace(name: &str) -> PathBuf {
    let workspace =
        std::env::temp_dir().join(format!("millracer-millrace-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    workspace
}

fn strings<const N: usize>(items: [&str; N]) -> Vec<String> {
    items.into_iter().map(ToOwned::to_owned).collect()
}
