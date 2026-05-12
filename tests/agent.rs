use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use millracer::agent::{
    AgentSession, MillraceLike, MillracerAgent, MonitorLike, PiLike, RunOptions,
};
use millracer::decision::Decision;
use millracer::intake::IntakeKind;
use millracer::monitor::MonitorEvent;
use millracer::scope::ScopedWorkItem;
use serde_json::{Map, Value, json};

#[derive(Debug, Default)]
struct FakePi {
    prompts: Vec<String>,
}

impl PiLike for FakePi {
    fn complete(
        &mut self,
        prompt: &str,
        _cwd: &Path,
        _timeout: Option<Duration>,
    ) -> millracer::MillracerResult<String> {
        self.prompts.push(prompt.to_owned());
        if prompt.contains("Return only JSON") {
            return Ok(r#"{"decision": "millrace", "why": "needs durable execution"}"#.to_owned());
        }
        if prompt.contains("Millrace emitted this terminal event") {
            return Ok("final answer".to_owned());
        }
        Ok("direct answer".to_owned())
    }
}

#[derive(Debug, Default)]
struct FakeMillrace {
    calls: Vec<String>,
    existing_scoped_intake: Option<PathBuf>,
    status_payload: Map<String, Value>,
}

impl MillraceLike for FakeMillrace {
    fn initialize(&mut self) -> millracer::MillracerResult<()> {
        self.calls.push("initialize".to_owned());
        Ok(())
    }

    fn validate(&mut self) -> millracer::MillracerResult<()> {
        self.calls.push("validate".to_owned());
        Ok(())
    }

    fn enqueue(
        &mut self,
        intake_kind: IntakeKind,
        task: &str,
        _scoped_work_item: Option<&ScopedWorkItem>,
    ) -> millracer::MillracerResult<PathBuf> {
        self.calls.push(format!("enqueue:{intake_kind}:{task}"));
        Ok(PathBuf::from(format!(
            "/tmp/ws/.millracer/intake/{intake_kind}.md"
        )))
    }

    fn find_existing_scoped_intake(
        &mut self,
        scoped_work_item: &ScopedWorkItem,
    ) -> millracer::MillracerResult<Option<PathBuf>> {
        self.calls
            .push(format!("find_existing:{}", scoped_work_item.item_id));
        Ok(self.existing_scoped_intake.clone())
    }

    fn start_daemon(&mut self) -> millracer::MillracerResult<()> {
        self.calls.push("start_daemon".to_owned());
        Ok(())
    }

    fn restart_daemon(&mut self) -> millracer::MillracerResult<()> {
        self.calls.push("restart_daemon".to_owned());
        Ok(())
    }

    fn stop_daemon(&mut self) -> millracer::MillracerResult<()> {
        self.calls.push("stop_daemon".to_owned());
        Ok(())
    }

    fn status(&mut self) -> millracer::MillracerResult<Map<String, Value>> {
        self.calls.push("status".to_owned());
        if !self.status_payload.is_empty() {
            return Ok(self.status_payload.clone());
        }
        let Value::Object(payload) = json!({"workspace": "/tmp/ws"}) else {
            unreachable!();
        };
        Ok(payload)
    }
}

#[derive(Debug)]
struct FakeMonitor {
    events: VecDeque<MonitorEvent>,
}

impl FakeMonitor {
    fn new(events: Vec<MonitorEvent>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl MonitorLike for FakeMonitor {
    fn wait(&mut self, _timeout_seconds: f64) -> millracer::MillracerResult<MonitorEvent> {
        Ok(self.events.pop_front().expect("monitor event"))
    }
}

#[test]
fn agent_routes_auto_decision_into_millrace_flow() {
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![MonitorEvent::new(
            "arbiter_complete",
            "/tmp/ws",
            "test",
        )]),
    );

    let result = agent
        .run(
            "Implement a multi-stage refactor",
            RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws")),
        )
        .expect("agent run");

    assert_eq!(result.route, "millrace");
    assert_eq!(result.intake_kind, "probe");
    assert_eq!(result.outcome, "completed");
    assert!(result.scoped_completion);
    assert_eq!(
        result.completion_evidence,
        vec![BTreeMap::from([
            ("kind".to_owned(), "arbiter_complete".to_owned()),
            ("reason".to_owned(), "test".to_owned()),
            ("workspace".to_owned(), "/tmp/ws".to_owned()),
        ])]
    );
    assert_eq!(
        result.decision,
        Decision::new("millrace", "needs durable execution")
    );
    assert_eq!(result.output, "final answer");
    let finalization_prompt = agent
        .pi
        .prompts
        .iter()
        .find(|prompt| prompt.contains("Millrace emitted this terminal event"))
        .expect("finalization prompt");
    assert!(finalization_prompt.contains("- outcome: completed"));
    assert!(finalization_prompt.contains("- scoped completion: true"));
    assert!(finalization_prompt.contains("Completion evidence:"));
    assert!(finalization_prompt.contains(r#""kind": "arbiter_complete""#));
    assert!(finalization_prompt.contains(r#""workspace": "/tmp/ws""#));
    assert!(
        finalization_prompt
            .contains("Treat daemon idle without scoped completion evidence as incomplete")
    );
    assert_eq!(
        agent.millrace.calls,
        [
            "initialize",
            "validate",
            "enqueue:probe:Implement a multi-stage refactor",
            "start_daemon",
            "stop_daemon",
            "status",
        ]
    );
}

#[test]
fn agent_reuses_existing_scoped_intake_without_duplicate_enqueue() {
    let intake_path = PathBuf::from("/tmp/ws/.millracer/intake/probe-old.md");
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.scoped_work_item = Some(ScopedWorkItem {
        item_id: "ITEM-123".to_owned(),
        title: None,
        source_queue: None,
        spec_path: None,
        completion_ref: Some("agent-impl-ITEM-123".to_owned()),
        constraints: Vec::new(),
    });
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace {
            existing_scoped_intake: Some(intake_path.clone()),
            ..FakeMillrace::default()
        },
        FakeMonitor::new(vec![MonitorEvent::new(
            "arbiter_complete",
            "/tmp/ws",
            "closure target closed",
        )]),
    );

    let result = agent
        .run("Process the selected scoped item", options)
        .expect("agent run");

    assert_eq!(result.task_path, Some(intake_path.display().to_string()));
    assert_eq!(result.outcome, "completed");
    assert!(result.scoped_completion);
    assert_eq!(
        result.completion_evidence[0]
            .get("reason")
            .map(String::as_str),
        Some("closure target closed")
    );
    assert_eq!(
        agent.millrace.calls,
        [
            "initialize",
            "validate",
            "find_existing:ITEM-123",
            "status",
            "start_daemon",
            "stop_daemon",
            "status",
        ]
    );
}

#[test]
fn agent_forced_direct_route_uses_pi_without_millrace_calls() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "direct".to_owned();
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![]),
    );

    let result = agent
        .run("Answer directly", options)
        .expect("agent direct run");

    assert_eq!(result.route, "direct");
    assert_eq!(result.output, "direct answer");
    assert_eq!(result.outcome, "completed");
    assert!(!result.scoped_completion);
    assert!(result.completion_evidence.is_empty());
    assert!(agent.millrace.calls.is_empty());
}

#[test]
fn agent_warns_when_decision_requests_custom_loop() {
    struct CustomLoopPi {
        prompts: Vec<String>,
    }

    impl PiLike for CustomLoopPi {
        fn complete(
            &mut self,
            prompt: &str,
            _cwd: &Path,
            _timeout: Option<Duration>,
        ) -> millracer::MillracerResult<String> {
            self.prompts.push(prompt.to_owned());
            if prompt.contains("Return only JSON") {
                return Ok(
                    r#"{"decision": "millrace", "why": "needs special topology", "custom_loop_needed": true, "mode": "default_pi"}"#
                        .to_owned(),
                );
            }
            Ok("final answer".to_owned())
        }
    }

    let mut agent = MillracerAgent::new(
        CustomLoopPi {
            prompts: Vec::new(),
        },
        FakeMillrace::default(),
        FakeMonitor::new(vec![MonitorEvent::new(
            "idle_no_work",
            "/tmp/ws",
            "daemon idle",
        )]),
    );

    let result = agent
        .run(
            "Use a custom workflow",
            RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws")),
        )
        .expect("agent run");

    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("custom Millrace loop"));
}

#[test]
fn agent_uses_forced_intake_override() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.intake = "task".to_owned();
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![MonitorEvent::new(
            "idle_no_work",
            "/tmp/ws",
            "daemon idle",
        )]),
    );

    let result = agent
        .run("Update behavior in a large pre-existing codebase.", options)
        .expect("agent run");

    assert_eq!(result.intake_kind, "task");
    assert!(
        agent
            .millrace
            .calls
            .contains(&"enqueue:task:Update behavior in a large pre-existing codebase.".to_owned())
    );
}

#[test]
fn agent_restarts_daemon_until_monitor_returns_terminal_event() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![
            MonitorEvent::new(
                "restart_needed",
                "/tmp/ws",
                "daemon stopped with queued work",
            ),
            MonitorEvent::new("idle_no_work", "/tmp/ws", "daemon idle with no work"),
        ]),
    );

    let result = agent
        .run("Process one scoped queue item", options)
        .expect("agent run");

    assert_eq!(
        result.event,
        Some(MonitorEvent::new(
            "idle_no_work",
            "/tmp/ws",
            "daemon idle with no work"
        ))
    );
    assert_eq!(result.outcome, "incomplete");
    assert!(!result.scoped_completion);
    assert!(result.completion_evidence.is_empty());
    assert!(agent.millrace.calls.contains(&"restart_daemon".to_owned()));
}

#[test]
fn agent_returns_restart_event_when_restart_limit_is_exhausted() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.max_daemon_restarts = 0;
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![MonitorEvent::new(
            "restart_needed",
            "/tmp/ws",
            "daemon stopped with queued work",
        )]),
    );

    let result = agent
        .run("Process one scoped queue item", options)
        .expect("agent run");

    assert_eq!(
        result.event,
        Some(MonitorEvent::new(
            "restart_needed",
            "/tmp/ws",
            "daemon stopped with queued work"
        ))
    );
    assert_eq!(result.outcome, "restart_needed");
    assert!(!result.scoped_completion);
    assert!(result.completion_evidence.is_empty());
    assert!(!agent.millrace.calls.contains(&"restart_daemon".to_owned()));
    assert!(agent.millrace.calls.contains(&"stop_daemon".to_owned()));
}

#[test]
fn agent_maps_blocked_and_crashed_events_without_scoped_completion() {
    for (kind, outcome, reason) in [
        ("blocked", "blocked", "blocked idle"),
        ("crashed", "crashed", "daemon stopped with active runs"),
    ] {
        let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
        options.route = "millrace".to_owned();
        let mut agent = MillracerAgent::new(
            FakePi::default(),
            FakeMillrace::default(),
            FakeMonitor::new(vec![MonitorEvent::new(kind, "/tmp/ws", reason)]),
        );

        let result = agent
            .run("Process one scoped queue item", options)
            .expect("agent run");

        assert_eq!(
            result.event,
            Some(MonitorEvent::new(kind, "/tmp/ws", reason))
        );
        assert_eq!(result.outcome, outcome);
        assert!(!result.scoped_completion);
        assert!(result.completion_evidence.is_empty());
    }
}

#[test]
fn agent_reports_existing_blocked_scoped_intake_without_starting_daemon() {
    let intake_path = PathBuf::from("/tmp/ws/.millracer/intake/probe-old.md");
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.scoped_work_item = Some(ScopedWorkItem {
        item_id: "ITEM-123".to_owned(),
        title: None,
        source_queue: None,
        spec_path: None,
        completion_ref: None,
        constraints: Vec::new(),
    });
    let Value::Object(status_payload) = json!({
        "workspace": "/tmp/ws",
        "current_failure_class": "recon_handoff_invalid",
        "latest_runtime_error_report_path": "/tmp/ws/runtime-errors/report.md",
        "active_run_count": 0,
        "execution_queue_depth": 0,
        "planning_queue_depth": 0,
        "learning_queue_depth": 0
    }) else {
        unreachable!();
    };
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace {
            existing_scoped_intake: Some(intake_path.clone()),
            status_payload,
            ..FakeMillrace::default()
        },
        FakeMonitor::new(vec![MonitorEvent::new(
            "arbiter_complete",
            "/tmp/ws",
            "should not be observed",
        )]),
    );

    let result = agent
        .run("Process the selected scoped item", options)
        .expect("agent run");

    assert_eq!(
        result.event,
        Some(MonitorEvent::new(
            "blocked",
            "/tmp/ws",
            "recon_handoff_invalid"
        ))
    );
    assert_eq!(result.task_path, Some(intake_path.display().to_string()));
    assert_eq!(result.outcome, "blocked");
    assert!(!result.scoped_completion);
    assert!(result.completion_evidence.is_empty());
    assert_eq!(
        agent.millrace.calls,
        ["initialize", "validate", "find_existing:ITEM-123", "status"]
    );
    let finalization_prompt = agent
        .pi
        .prompts
        .iter()
        .find(|prompt| prompt.contains("Millrace emitted this terminal event"))
        .expect("finalization prompt");
    assert!(finalization_prompt.contains("- outcome: blocked"));
    assert!(finalization_prompt.contains("- scoped completion: false"));
}

#[test]
fn agent_respects_keep_daemon_after_terminal_event() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.keep_daemon = true;
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![MonitorEvent::new(
            "idle_no_work",
            "/tmp/ws",
            "daemon idle",
        )]),
    );

    agent
        .run("Process one scoped queue item", options)
        .expect("agent run");

    assert!(!agent.millrace.calls.contains(&"stop_daemon".to_owned()));
    assert!(agent.millrace.calls.contains(&"status".to_owned()));
}

#[test]
fn agent_surfaces_progress_events_and_notifies_pi() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![
            MonitorEvent::new("stage_progress", "/tmp/ws", "updater update complete"),
            MonitorEvent::new("idle_no_work", "/tmp/ws", "daemon idle"),
        ]),
    );

    let result = agent
        .run("Process one scoped queue item", options)
        .expect("agent run");

    assert_eq!(
        result.progress_events,
        vec![MonitorEvent::new(
            "stage_progress",
            "/tmp/ws",
            "updater update complete"
        )]
    );
    assert!(
        agent
            .pi
            .prompts
            .iter()
            .any(|prompt| prompt.contains("Millrace reported this progress event"))
    );
}

#[test]
fn agent_can_ignore_progress_prompts_when_notifications_are_disabled() {
    let mut options = RunOptions::new(PathBuf::from("/tmp/ws"), PathBuf::from("/tmp/ws"));
    options.route = "millrace".to_owned();
    options.notify_terminal_stages = false;
    let mut agent = MillracerAgent::new(
        FakePi::default(),
        FakeMillrace::default(),
        FakeMonitor::new(vec![
            MonitorEvent::new("stage_progress", "/tmp/ws", "updater update complete"),
            MonitorEvent::new("idle_no_work", "/tmp/ws", "daemon idle"),
        ]),
    );

    let result = agent
        .run("Process one scoped queue item", options)
        .expect("agent run");

    assert!(result.progress_events.is_empty());
    assert!(
        agent
            .pi
            .prompts
            .iter()
            .all(|prompt| !prompt.contains("Millrace reported this progress event"))
    );
}
