use std::collections::VecDeque;

use millracer::benchmark::{RunResult, render_benchmark_result};
use millracer::decision::Decision;
use millracer::monitor::{DaemonMonitor, MonitorEvent, classify_status};
use serde_json::{Map, Value, json};

#[test]
fn classify_status_detects_blocked_runtime() {
    let event = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "blocked_idle": true,
            "current_failure_class": "runner_timeout"
        })),
        true,
    );

    assert_eq!(
        event,
        Some(MonitorEvent::new("blocked", "/tmp/ws", "runner_timeout"))
    );
}

#[test]
fn classify_status_detects_blocked_idle_without_failure_class() {
    let event = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "blocked_idle": true,
            "current_failure_class": ""
        })),
        true,
    );

    assert_eq!(
        event,
        Some(MonitorEvent::new("blocked", "/tmp/ws", "blocked idle"))
    );
}

#[test]
fn classify_status_detects_stopped_daemon_with_queued_work() {
    let event = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "process_running": false,
            "active_stage": "none",
            "active_run_count": 0,
            "execution_queue_depth": 1,
            "planning_queue_depth": 0,
            "learning_queue_depth": 0,
            "runtime_ownership_lock": "stale"
        })),
        true,
    );

    assert_eq!(
        event,
        Some(MonitorEvent::new(
            "restart_needed",
            "/tmp/ws",
            "daemon stopped with queued work and stale runtime ownership lock"
        ))
    );
}

#[test]
fn classify_status_detects_arbiter_marker_and_closed_closure_target() {
    let marker = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "process_running": true,
            "planning_status_marker": "  ### ARBITER_COMPLETE\n",
            "active_run_count": 0
        })),
        true,
    );
    let closure = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "process_running": true,
            "closure_target_root_spec_id": "spec-001",
            "closure_target_open": false,
            "active_run_count": 0
        })),
        true,
    );

    assert_eq!(
        marker,
        Some(MonitorEvent::new(
            "arbiter_complete",
            "/tmp/ws",
            "arbiter marker"
        ))
    );
    assert_eq!(
        closure,
        Some(MonitorEvent::new(
            "arbiter_complete",
            "/tmp/ws",
            "closure target closed"
        ))
    );
}

#[test]
fn classify_status_treats_updater_complete_with_remaining_work_as_progress() {
    let payload = object(json!({
        "workspace": "/tmp/ws",
        "process_running": true,
        "active_stage": "none",
        "active_run_count": 0,
        "execution_queue_depth": 0,
        "planning_queue_depth": 1,
        "learning_queue_depth": 0,
        "execution_status_marker": "### UPDATE_COMPLETE"
    }));

    assert_eq!(
        classify_status(&payload, true),
        Some(MonitorEvent::new(
            "stage_progress",
            "/tmp/ws",
            "updater update complete"
        ))
    );
    assert_eq!(classify_status(&payload, false), None);
}

#[test]
fn classify_status_detects_running_daemon_with_drained_work() {
    let event = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "process_running": true,
            "active_stage": "none",
            "active_run_count": 0,
            "execution_queue_depth": 0,
            "planning_queue_depth": 0,
            "learning_queue_depth": 0,
            "execution_status_marker": "### UPDATE_COMPLETE"
        })),
        true,
    );

    assert_eq!(
        event,
        Some(MonitorEvent::new(
            "complete",
            "/tmp/ws",
            "daemon idle with no work"
        ))
    );
}

#[test]
fn classify_status_detects_crashed_daemon_with_active_runs() {
    let event = classify_status(
        &object(json!({
            "workspace": "/tmp/ws",
            "process_running": false,
            "active_run_count": "2",
            "execution_queue_depth": 0,
            "planning_queue_depth": 0,
            "learning_queue_depth": 0
        })),
        true,
    );

    assert_eq!(
        event,
        Some(MonitorEvent::new(
            "crashed",
            "/tmp/ws",
            "daemon stopped with active runs"
        ))
    );
}

#[test]
fn monitor_wait_returns_first_terminal_event() {
    let mut statuses = statuses([
        json!({"workspace": "/tmp/ws", "process_running": true, "active_run_count": 1}),
        json!({"workspace": "/tmp/ws", "planning_status_marker": "### ARBITER_COMPLETE"}),
    ]);
    let mut monitor = DaemonMonitor::with_sleep(
        move || Ok(statuses.pop_front().expect("status")),
        0.0,
        |_| {},
    );

    assert_eq!(monitor.wait(1.0).expect("event").kind, "arbiter_complete");
}

#[test]
fn monitor_wait_times_out_with_workspace_context() {
    let statuses = statuses([json!({
        "workspace": "/tmp/ws",
        "process_running": true,
        "active_run_count": 1
    })]);
    let mut monitor = DaemonMonitor::with_sleep(
        move || Ok(statuses.front().expect("status").clone()),
        0.0,
        |_| {},
    );

    let error = monitor.wait(0.0).expect_err("timeout");
    assert_eq!(
        error.to_string(),
        "timed out waiting for Millrace daemon event in /tmp/ws"
    );
}

#[test]
fn monitor_suppresses_duplicate_progress_events_until_terminal_event() {
    let mut statuses = statuses([
        progress_status("ITEM-1", 1),
        progress_status("ITEM-1", 1),
        json!({
            "workspace": "/tmp/ws",
            "process_running": true,
            "active_stage": "none",
            "active_run_count": 0,
            "execution_queue_depth": 0,
            "planning_queue_depth": 0,
            "learning_queue_depth": 0
        }),
    ]);
    let mut monitor = DaemonMonitor::with_sleep(
        move || Ok(statuses.pop_front().expect("status")),
        0.0,
        |_| {},
    );

    assert_eq!(monitor.wait(1.0).expect("progress").kind, "stage_progress");
    assert_eq!(monitor.wait(1.0).expect("complete").kind, "complete");
}

#[test]
fn monitor_preserves_distinct_terminal_stage_progress_notifications() {
    let mut statuses = statuses([progress_status("ITEM-1", 1), progress_status("ITEM-2", 1)]);
    let mut monitor = DaemonMonitor::with_sleep(
        move || Ok(statuses.pop_front().expect("status")),
        0.0,
        |_| {},
    );

    assert_eq!(monitor.wait(1.0).expect("first").kind, "stage_progress");
    assert_eq!(monitor.wait(1.0).expect("second").kind, "stage_progress");
}

#[test]
fn monitor_events_are_jsonable_and_fit_run_result_boundary() {
    let event = MonitorEvent::new("stage_progress", "/tmp/ws", "updater update complete");
    let event_json = serde_json::to_value(&event).expect("monitor event json");
    assert_eq!(event_json["kind"], "stage_progress");
    assert_eq!(event_json["workspace"], "/tmp/ws");

    let raw = render_benchmark_result(&RunResult {
        route: "millrace".to_owned(),
        intake_kind: "task".to_owned(),
        intake_signals: Vec::new(),
        decision: Decision::new("millrace", "test"),
        output: "done".to_owned(),
        event: Some(MonitorEvent::new("complete", "/tmp/ws", "daemon idle")),
        task_path: Some("/tmp/ws/.millracer/intake/task.md".to_owned()),
        status: Some(json!({"workspace": "/tmp/ws"})),
        warnings: Vec::new(),
        scoped_work_item: None,
        progress_events: vec![event],
        task: "do work".to_owned(),
        workspace: "/tmp/ws".to_owned(),
        cwd: "/tmp/ws".to_owned(),
        pi_session: "rpc".to_owned(),
        millrace_mode: "default_pi".to_owned(),
        notify_terminal_stages: true,
    })
    .expect("run result json");
    let payload: Value = serde_json::from_str(&raw).expect("json payload");

    assert_eq!(payload["event"]["kind"], "complete");
    assert_eq!(payload["progress_events"][0]["kind"], "stage_progress");
    assert_eq!(
        payload["progress_events"][0]["reason"],
        "updater update complete"
    );
}

fn progress_status(active_work_item_id: &str, planning_queue_depth: i32) -> Value {
    json!({
        "workspace": "/tmp/ws",
        "process_running": true,
        "active_stage": "none",
        "active_run_count": 0,
        "active_work_item_id": active_work_item_id,
        "execution_queue_depth": 0,
        "planning_queue_depth": planning_queue_depth,
        "learning_queue_depth": 0,
        "execution_status_marker": "### UPDATE_COMPLETE"
    })
}

fn statuses<const N: usize>(values: [Value; N]) -> VecDeque<Map<String, Value>> {
    values.into_iter().map(object).collect()
}

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(payload) = value else {
        panic!("test status must be an object");
    };
    payload
}
