use std::path::PathBuf;

use millracer::benchmark::{
    MonitorEvent, RunResult, parse_benchmark_request, render_benchmark_result,
};
use millracer::decision::Decision;
use millracer::scope::ScopedWorkItem;
use serde_json::Value;

#[test]
fn parse_benchmark_request_preserves_scoped_work_item() {
    let request = parse_benchmark_request(
        r#"
        {
          "task": "Implement the selected queue item only.",
          "workspace": "/tmp/ws",
          "intake_kind": "probe",
          "scoped_work_item": {
            "item_id": "M06",
            "title": "Array API label encoder support",
            "source_queue": "/queue.md",
            "spec_path": "/srs/M06.md",
            "completion_ref": "agent-impl-M06",
            "constraints": ["Do not implement any other queue item."]
          }
        }
        "#,
    )
    .expect("valid benchmark request");

    assert_eq!(request.workspace, Some(PathBuf::from("/tmp/ws")));
    assert_eq!(request.intake_kind.as_deref(), Some("probe"));
    assert_eq!(
        request.scoped_work_item,
        Some(ScopedWorkItem {
            item_id: "M06".to_owned(),
            title: Some("Array API label encoder support".to_owned()),
            source_queue: Some("/queue.md".to_owned()),
            spec_path: Some("/srs/M06.md".to_owned()),
            completion_ref: Some("agent-impl-M06".to_owned()),
            constraints: vec!["Do not implement any other queue item.".to_owned()],
        })
    );
}

#[test]
fn parse_benchmark_request_accepts_prompt_instructions_and_scope_aliases() {
    let prompt = parse_benchmark_request(r#"{"prompt":"do work","work_item":{"id":"A1"}}"#)
        .expect("prompt alias");
    let instructions =
        parse_benchmark_request(r#"{"instructions":"do work","scope":{"item_id":"A2"}}"#)
            .expect("instructions alias");

    assert_eq!(prompt.task, "do work");
    assert_eq!(
        prompt
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("A1")
    );
    assert_eq!(instructions.task, "do work");
    assert_eq!(
        instructions
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("A2")
    );
}

#[test]
fn parse_benchmark_request_uses_truthy_intake_fallback() {
    let metadata_fallback = parse_benchmark_request(
        r#"{"task":"Do work","intake_kind":null,"metadata":{"intake_kind":"task"}}"#,
    )
    .expect("metadata intake fallback");
    let invalid = parse_benchmark_request(r#"{"task":"Fix src/lib.rs","intake_kind":"whatever"}"#)
        .expect("invalid intake normalizes away");

    assert_eq!(metadata_fallback.intake_kind.as_deref(), Some("task"));
    assert_eq!(invalid.intake_kind, None);
}

#[test]
fn parse_benchmark_request_skips_unusable_scoped_work_aliases() {
    let work_item_fallback = parse_benchmark_request(
        r#"{"task":"Do scoped work","scoped_work_item":null,"work_item":{"id":"A1"}}"#,
    )
    .expect("work item fallback");
    let metadata_fallback = parse_benchmark_request(
        r#"
        {
          "task": "Do scoped work",
          "scoped_work_item": {},
          "work_item": "bad",
          "scope": {"title": "missing id"},
          "metadata": {"scoped_work_item": {"item_id": "M9"}}
        }
        "#,
    )
    .expect("metadata scoped fallback");

    assert_eq!(
        work_item_fallback
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("A1")
    );
    assert_eq!(
        metadata_fallback
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("M9")
    );
}

#[test]
fn render_benchmark_result_includes_scoped_work_item() {
    let raw = render_benchmark_result(&RunResult {
        route: "millrace".to_owned(),
        intake_kind: "probe".to_owned(),
        intake_signals: vec!["large pre-existing codebase".to_owned()],
        decision: Decision::new("millrace", "test"),
        output: "done".to_owned(),
        event: Some(MonitorEvent {
            kind: "complete".to_owned(),
            workspace: "/tmp/ws".to_owned(),
            reason: "daemon idle".to_owned(),
        }),
        task_path: Some("/tmp/ws/.millracer/intake/probe.md".to_owned()),
        status: Some(serde_json::json!({"workspace": "/tmp/ws"})),
        warnings: Vec::new(),
        scoped_work_item: Some(ScopedWorkItem {
            item_id: "ITEM-123".to_owned(),
            title: None,
            source_queue: None,
            spec_path: None,
            completion_ref: None,
            constraints: Vec::new(),
        }),
        progress_events: Vec::new(),
        task: "do work".to_owned(),
        workspace: "/tmp/ws".to_owned(),
        cwd: "/tmp/ws".to_owned(),
        pi_session: "rpc".to_owned(),
        millrace_mode: "default_pi".to_owned(),
        notify_terminal_stages: true,
    })
    .expect("json render");

    let payload: Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(payload["scoped_work_item"]["item_id"], "ITEM-123");
    assert_eq!(payload["intake_kind"], "probe");
    assert_eq!(payload["intake_signals"][0], "large pre-existing codebase");
    assert_eq!(payload["decision"]["mode"], "default_pi");
    assert_eq!(payload["event"]["kind"], "complete");
    assert_eq!(payload["task_path"], "/tmp/ws/.millracer/intake/probe.md");
}
