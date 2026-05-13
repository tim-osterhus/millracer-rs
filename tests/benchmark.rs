use std::collections::BTreeMap;
use std::path::PathBuf;

use millracer::benchmark::{
    MonitorEvent, RunResult, ops_result_to_legacy_json, parse_benchmark_request,
    parse_legacy_request_as_ops, render_benchmark_result,
};
use millracer::decision::Decision;
use millracer::ops_models::{Completion, OpsResult, SCHEMA_VERSION, WorkspaceRef};
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
            kind: "idle_no_work".to_owned(),
            workspace: "/tmp/ws".to_owned(),
            reason: "daemon idle".to_owned(),
        }),
        task_path: Some("/tmp/ws/.millracer/intake/probe.md".to_owned()),
        status: Some(serde_json::json!({"workspace": "/tmp/ws"})),
        warnings: Vec::new(),
        outcome: "completed".to_owned(),
        scoped_completion: true,
        completion_evidence: vec![BTreeMap::from([
            ("kind".to_owned(), "arbiter_complete".to_owned()),
            ("reason".to_owned(), "closed".to_owned()),
            ("workspace".to_owned(), "/tmp/ws".to_owned()),
        ])],
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
    assert_eq!(payload["event"]["kind"], "idle_no_work");
    assert_eq!(payload["task_path"], "/tmp/ws/.millracer/intake/probe.md");
    assert_eq!(payload["outcome"], "completed");
    assert_eq!(payload["scoped_completion"], true);
    assert_eq!(
        payload["completion_evidence"][0]["kind"],
        "arbiter_complete"
    );
    assert_eq!(payload["completion_evidence"][0]["reason"], "closed");
    assert_eq!(payload["completion_evidence"][0]["workspace"], "/tmp/ws");
}

#[test]
fn legacy_request_maps_to_ops_request() {
    let request = parse_legacy_request_as_ops(
        include_str!("fixtures/ops/legacy_request.json"),
        Some("req-legacy-001"),
    )
    .expect("legacy request");

    assert_eq!(request.schema_version, SCHEMA_VERSION);
    assert_eq!(request.request_id, "req-legacy-001");
    assert_eq!(request.source.kind, "benchmark_compat");
    assert_eq!(request.action, "enqueue");
    assert_eq!(request.route_preference, "millrace");
    assert_eq!(request.intake_preference, "probe");
    assert_eq!(request.input["kind"], "legacy_benchmark");
    assert_eq!(
        request.input["text"],
        "Implement the selected queue item only."
    );
    assert_eq!(
        request
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("ITEM-123")
    );
    assert_eq!(
        request.metadata["legacy_request"]["scoped_work_item"]["completion_ref"],
        "complete-ITEM-123"
    );
}

#[test]
fn ops_result_can_render_legacy_json() {
    let raw = ops_result_to_legacy_json(&OpsResult {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: "req-legacy-001".to_owned(),
        status: "incomplete".to_owned(),
        action: "enqueue".to_owned(),
        workspace_ref: WorkspaceRef {
            workspace_id: None,
            root_path: Some("/tmp/ws".to_owned()),
            display_name: None,
            runtime_kind: "local".to_owned(),
            mode: None,
            environment: None,
        },
        started_at: "2026-05-12T00:00:00+00:00".to_owned(),
        finished_at: "2026-05-12T00:00:01+00:00".to_owned(),
        warnings: Vec::new(),
        errors: Vec::new(),
        result: serde_json::Map::from_iter([(
            "output".to_owned(),
            Value::String("not complete".to_owned()),
        )]),
        route: Some("millrace".to_owned()),
        intake_kind: Some("probe".to_owned()),
        scoped_work_item: Some(ScopedWorkItem {
            item_id: "ITEM-123".to_owned(),
            title: None,
            source_queue: None,
            spec_path: None,
            completion_ref: None,
            constraints: Vec::new(),
        }),
        completion: Some(Completion {
            outcome: "incomplete".to_owned(),
            scoped_completion: false,
            evidence_summary: Vec::new(),
            missing_evidence: Vec::new(),
            verification_status: "unverified".to_owned(),
            terminal_outcome_ref: None,
        }),
        evidence_refs: Vec::new(),
        event_cursor: None,
        session_ref: None,
        raw_compat: Some(serde_json::Map::from_iter([(
            "route".to_owned(),
            Value::String("millrace".to_owned()),
        )])),
    })
    .expect("legacy render");

    let payload: Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(payload["status"], "incomplete");
    assert_eq!(payload["completion"]["scoped_completion"], false);
    assert_eq!(payload["raw_compat"]["route"], "millrace");
}
