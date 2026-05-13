use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use millracer::agent::{AgentSession, RunOptions};
use millracer::benchmark::RunResult;
use millracer::decision::Decision;
use millracer::monitor::MonitorEvent;
use millracer::ops_models::{
    JsonObject, OpsRequest, SCHEMA_VERSION, SourceRef, WorkspaceRef, parse_ops_request,
};
use millracer::ops_service::{OpsRuntime, OpsService};
use millracer::sessions::SessionStore;
use millracer::workspaces::{WorkspaceRecord, WorkspaceRegistry};
use millracer::{MillracerError, MillracerResult};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct FakeRuntime {
    status_payload: JsonObject,
}

impl OpsRuntime for FakeRuntime {
    fn status(&mut self) -> MillracerResult<JsonObject> {
        Ok(self.status_payload.clone())
    }
}

#[derive(Debug)]
struct FailingRuntime;

impl OpsRuntime for FailingRuntime {
    fn status(&mut self) -> MillracerResult<JsonObject> {
        Err(MillracerError::message("millrace status failed"))
    }
}

#[derive(Debug)]
struct FakeAgent {
    result: RunResult,
    calls: Rc<RefCell<Vec<(String, RunOptions)>>>,
}

impl AgentSession for FakeAgent {
    fn run(&mut self, task: &str, options: RunOptions) -> MillracerResult<RunResult> {
        self.calls.borrow_mut().push((task.to_owned(), options));
        Ok(self.result.clone())
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "millracer-ops-service-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn request(action: &str, workspace_root: Option<&Path>) -> OpsRequest {
    OpsRequest {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: format!("req-{action}"),
        action: action.to_owned(),
        workspace_ref: WorkspaceRef {
            workspace_id: None,
            root_path: workspace_root.map(|path| path.display().to_string()),
            display_name: None,
            runtime_kind: "local".to_owned(),
            mode: None,
            environment: None,
        },
        source: SourceRef {
            kind: "cli".to_owned(),
            surface: None,
            adapter_id: None,
            conversation_id: None,
            message_id: None,
            parent_request_id: None,
            trace_ref: None,
        },
        input: JsonObject::from_iter([(
            "payload".to_owned(),
            json!({ "kind": "structured_action" }),
        )]),
        client_name: None,
        client_version: None,
        actor: None,
        route_preference: "auto".to_owned(),
        intake_preference: "auto".to_owned(),
        scoped_work_item: None,
        expected_evidence: Vec::new(),
        context_refs: Vec::new(),
        options: JsonObject::new(),
        metadata: JsonObject::new(),
        idempotency_key: None,
    }
}

fn run_result(outcome: &str, scoped_completion: bool) -> RunResult {
    RunResult {
        route: "millrace".to_owned(),
        intake_kind: "task".to_owned(),
        intake_signals: vec!["forced_task".to_owned()],
        decision: Decision::new("millrace", "forced"),
        output: "delegated".to_owned(),
        event: Some(MonitorEvent::new(
            if scoped_completion {
                "scoped_complete"
            } else {
                "idle_no_work"
            },
            "/tmp/ws",
            "done",
        )),
        task_path: Some("/tmp/ws/.millracer/intake/task.md".to_owned()),
        status: Some(json!({"workspace": "/tmp/ws"})),
        warnings: vec!["custom loop warning".to_owned()],
        outcome: outcome.to_owned(),
        scoped_completion,
        completion_evidence: if scoped_completion {
            vec![BTreeMap::from([
                ("kind".to_owned(), "scoped_complete".to_owned()),
                ("workspace".to_owned(), "/tmp/ws".to_owned()),
                ("reason".to_owned(), "done".to_owned()),
            ])]
        } else {
            Vec::new()
        },
        scoped_work_item: None,
        progress_events: Vec::new(),
        task: "Implement the selected packet only.".to_owned(),
        workspace: "/tmp/ws".to_owned(),
        cwd: "/tmp/ws".to_owned(),
        pi_session: "rpc".to_owned(),
        millrace_mode: "learning_codex".to_owned(),
        notify_terminal_stages: true,
    }
}

#[test]
fn structured_status_uses_runtime_without_agent() {
    let workspace = temp_dir("status");
    let workspace_for_runtime = workspace.clone();
    let runtime_calls = Rc::new(RefCell::new(0usize));
    let calls = Rc::clone(&runtime_calls);
    let mut service = OpsService::new(move |_| {
        *calls.borrow_mut() += 1;
        let Value::Object(status_payload) = json!({"workspace": workspace_for_runtime.display().to_string(), "process_running": false})
        else {
            unreachable!();
        };
        FakeRuntime { status_payload }
    });
    let result = service.handle(&request("status", Some(&workspace)));

    assert_eq!(result.status, "succeeded");
    assert_eq!(result.action, "status");
    assert_eq!(result.route.as_deref(), Some("status_only"));
    assert_eq!(result.result["process_running"], false);
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.verification_status.as_str()),
        Some("not_applicable")
    );
    assert_eq!(*runtime_calls.borrow(), 1);
}

#[test]
fn runtime_status_failure_maps_to_machine_error() {
    let workspace = temp_dir("status-fail");
    let mut service = OpsService::new(|_| FailingRuntime);

    let result = service.handle(&request("status", Some(&workspace)));

    assert_eq!(result.status, "failed");
    assert_eq!(result.errors[0].code, "runtime_command_failed");
    assert!(result.errors[0].message.contains("millrace status failed"));
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.verification_status.as_str()),
        Some("unverified")
    );
}

#[test]
fn unsupported_action_and_unresolved_workspace_return_machine_errors_without_runtime() {
    let workspace = temp_dir("unsupported");
    let runtime_calls = Rc::new(RefCell::new(0usize));
    let calls = Rc::clone(&runtime_calls);
    let mut service = OpsService::new(move |_| {
        *calls.borrow_mut() += 1;
        FakeRuntime {
            status_payload: JsonObject::new(),
        }
    });

    let unsupported = service.handle(&request("approve", Some(&workspace)));
    service.cwd = None;
    let unresolved = service.handle(&request("status", None));

    assert_eq!(unsupported.status, "failed");
    assert_eq!(unsupported.errors[0].code, "unsupported_action");
    assert_eq!(unresolved.status, "failed");
    assert_eq!(unresolved.errors[0].code, "workspace_unresolved");
    assert_eq!(*runtime_calls.borrow(), 0);
}

#[test]
fn enqueue_maps_run_result_completion_and_agent_options() {
    let agent_calls = Rc::new(RefCell::new(Vec::new()));
    let result_template = run_result("incomplete", false);
    let calls = Rc::clone(&agent_calls);
    let mut service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    })
    .with_agent_factory(move |_resolution, _runtime| FakeAgent {
        result: result_template.clone(),
        calls: Rc::clone(&calls),
    });
    let request = parse_ops_request(include_str!("fixtures/ops/enqueue_request.json"))
        .expect("enqueue request fixture");

    let result = service.handle(&request);

    assert_eq!(result.status, "incomplete");
    assert_eq!(result.route.as_deref(), Some("millrace"));
    assert_eq!(result.intake_kind.as_deref(), Some("task"));
    assert_eq!(result.warnings[0].code, "runtime_warning");
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.missing_evidence.as_slice()),
        Some(["positive scoped completion evidence".to_owned()].as_slice())
    );
    assert_eq!(
        result
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("WP-002A")
    );
    assert_eq!(result.result["task"], "Implement the selected packet only.");
    assert_eq!(result.raw_compat.as_ref().unwrap()["output"], "delegated");

    let calls = agent_calls.borrow();
    assert_eq!(calls[0].0, "Implement the selected packet only.");
    assert_eq!(calls[0].1.route, "millrace");
    assert_eq!(calls[0].1.intake, "task");
    assert_eq!(
        calls[0]
            .1
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("WP-002A")
    );
    assert_eq!(calls[0].1.millrace_mode, "learning_codex");
}

#[test]
fn enqueue_extracts_nested_payload_text_with_python_truthiness() {
    let workspace = temp_dir("nested");
    let agent_calls = Rc::new(RefCell::new(Vec::new()));
    let calls = Rc::clone(&agent_calls);
    let mut service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    })
    .with_agent_factory(move |_resolution, _runtime| FakeAgent {
        result: run_result("completed", true),
        calls: Rc::clone(&calls),
    });
    let mut request = request("enqueue", Some(&workspace));
    request.route_preference = "invalid-route".to_owned();
    request.intake_preference = "bad-intake".to_owned();
    request.input = JsonObject::from_iter([(
        "payload".to_owned(),
        json!({"task": "", "text": "Nested delegated task"}),
    )]);

    let result = service.handle(&request);

    assert_eq!(result.status, "succeeded");
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.verification_status.as_str()),
        Some("verified")
    );
    assert_eq!(agent_calls.borrow()[0].0, "Nested delegated task");
    assert_eq!(agent_calls.borrow()[0].1.route, "auto");
    assert_eq!(agent_calls.borrow()[0].1.intake, "auto");
}

#[test]
fn enqueue_rejects_whitespace_task_before_payload_text_fallback() {
    let workspace = temp_dir("truthy-space");
    let mut service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    })
    .with_agent_factory(|_resolution, _runtime| FakeAgent {
        result: run_result("completed", true),
        calls: Rc::new(RefCell::new(Vec::new())),
    });
    let mut request = request("enqueue", Some(&workspace));
    request.input = JsonObject::from_iter([(
        "payload".to_owned(),
        json!({"task": "   ", "text": "Would be valid"}),
    )]);

    let result = service.handle(&request);

    assert_eq!(result.status, "failed");
    assert_eq!(result.errors[0].code, "invalid_input");
}

#[test]
fn enqueue_maps_terminal_outcomes_to_ops_statuses() {
    let workspace = temp_dir("statuses");
    let cases = VecDeque::from([
        run_result("completed", true),
        run_result("blocked", false),
        run_result("crashed", false),
        run_result("incomplete", false),
        run_result("completed", false),
    ]);
    let cases = Rc::new(RefCell::new(cases));
    let mut service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    })
    .with_agent_factory({
        let cases = Rc::clone(&cases);
        move |_resolution, _runtime| FakeAgent {
            result: cases.borrow_mut().pop_front().expect("case"),
            calls: Rc::new(RefCell::new(Vec::new())),
        }
    });
    let mut request = request("enqueue", Some(&workspace));
    request.input = JsonObject::from_iter([("text".to_owned(), Value::String("task".to_owned()))]);

    assert_eq!(service.handle(&request).status, "succeeded");
    assert_eq!(service.handle(&request).status, "blocked");
    assert_eq!(service.handle(&request).status, "failed");
    assert_eq!(service.handle(&request).status, "incomplete");
    assert_eq!(service.handle(&request).status, "failed");
}

#[test]
fn inspect_only_actions_use_registry_and_session_store() -> MillracerResult<()> {
    let workspace = temp_dir("inspect");
    let sessions_path = workspace.join("sessions.json");
    let mut record = WorkspaceRecord::new("millrace-os", &workspace);
    record.display_name = Some("Millrace OS".to_owned());
    record.default_mode = Some("learning_codex".to_owned());
    record.tags = vec!["local".to_owned()];
    let mut service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    });
    service.registry = WorkspaceRegistry {
        records: BTreeMap::from([("millrace-os".to_owned(), record)]),
        default_workspace_id: Some("millrace-os".to_owned()),
    };
    service.session_store = Some(SessionStore::new(&sessions_path));

    let workspaces = service.handle(&request("list_workspaces", Some(&workspace)));
    assert_eq!(workspaces.status, "succeeded");
    assert_eq!(workspaces.route.as_deref(), Some("inspect_only"));
    assert_eq!(
        workspaces.result["workspaces"][0]["workspace_id"],
        "millrace-os"
    );
    assert_eq!(workspaces.result["workspaces"][0]["is_default"], true);

    let mut select = request("select_workspace", Some(&workspace));
    select.input = JsonObject::from_iter([(
        "payload".to_owned(),
        json!({"workspace_id": "millrace-os", "session_id": "local"}),
    )]);
    let selected = service.handle(&select);
    assert_eq!(selected.status, "succeeded");
    assert_eq!(selected.session_ref.as_deref(), Some("local"));

    let sessions = service.handle(&request("list_sessions", Some(&workspace)));
    assert_eq!(sessions.result["sessions"][0]["session_id"], "local");
    assert_eq!(
        sessions.result["sessions"][0]["selected_workspace_id"],
        "millrace-os"
    );

    let mut inspect = request("inspect_session", Some(&workspace));
    inspect.input = JsonObject::from_iter([("payload".to_owned(), json!({"session_id": "local"}))]);
    let inspected = service.handle(&inspect);
    assert_eq!(
        inspected.result["session"]["selected_mode"],
        "learning_codex"
    );
    Ok(())
}

#[test]
fn preview_events_exposes_request_accepted_frame() {
    let workspace = temp_dir("preview");
    let service = OpsService::new(|_| FakeRuntime {
        status_payload: JsonObject::new(),
    });

    let frames = service.preview_events(&request("status", Some(&workspace)));

    assert_eq!(frames[0].schema_version, SCHEMA_VERSION);
    assert_eq!(frames[0].event_type, "request.accepted");
    assert_eq!(frames[0].message, "Request accepted.");
    assert_eq!(frames[0].payload["action"], "status");
    assert_eq!(frames[0].cursor.as_deref(), Some("1"));
}
