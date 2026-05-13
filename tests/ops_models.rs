use millracer::ops_models::{
    Completion, ErrorRecord, OpsEventFrame, SCHEMA_VERSION, WarningRecord, parse_ops_request,
    parse_ops_result, render_ops_event_frame, render_ops_request, render_ops_result,
};
use millracer::scope::ScopedWorkItem;
use serde_json::{Map, Value, json};

#[test]
fn ops_request_roundtrips_status_fixture() {
    let request = parse_ops_request(include_str!("fixtures/ops/status_request.json"))
        .expect("status request fixture");

    assert_eq!(request.schema_version, SCHEMA_VERSION);
    assert_eq!(request.request_id, "req-status-001");
    assert_eq!(request.action, "status");
    assert_eq!(
        request.workspace_ref.workspace_id.as_deref(),
        Some("millrace-os")
    );
    assert_eq!(
        request.workspace_ref.mode.as_deref(),
        Some("learning_codex")
    );
    assert_eq!(request.source.kind, "mission_control");
    assert_eq!(
        request.actor.as_ref().map(|actor| actor.kind.as_str()),
        Some("local_user")
    );

    let rendered = render_ops_request(&request);
    assert_eq!(rendered["schema_version"], SCHEMA_VERSION);
    assert_eq!(rendered["workspace_ref"]["runtime_kind"], "local");
    assert!(rendered.get("client_version").is_none());
}

#[test]
fn ops_result_roundtrips_status_fixture() {
    let result =
        parse_ops_result(include_str!("fixtures/ops/status_result.json")).expect("status result");

    assert_eq!(result.status, "succeeded");
    assert_eq!(result.route.as_deref(), Some("status_only"));
    assert!(result.completion.is_some());
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.verification_status.as_str()),
        Some("not_applicable")
    );

    let rendered = render_ops_result(&result);
    assert_eq!(
        rendered["completion"]["verification_status"],
        "not_applicable"
    );
    assert_eq!(rendered["completion"]["terminal_outcome_ref"], Value::Null);
}

#[test]
fn ops_request_preserves_enqueue_scoped_fixture_fields() {
    let request = parse_ops_request(include_str!("fixtures/ops/enqueue_request.json"))
        .expect("enqueue request fixture");

    assert_eq!(request.route_preference, "millrace");
    assert_eq!(request.intake_preference, "task");
    assert_eq!(
        request.scoped_work_item,
        Some(ScopedWorkItem {
            item_id: "WP-002A".to_owned(),
            title: Some("Millracer ops contract skeleton".to_owned()),
            source_queue: None,
            spec_path: None,
            completion_ref: None,
            constraints: vec!["Do not implement unrelated packets.".to_owned()],
        })
    );
    assert_eq!(
        request.expected_evidence,
        vec![
            "changed file list".to_owned(),
            "tests run".to_owned(),
            "terminal outcome".to_owned()
        ]
    );

    let rendered = render_ops_request(&request);
    assert_eq!(rendered["scoped_work_item"]["item_id"], "WP-002A");
    assert_eq!(rendered["workspace_ref"]["runtime_kind"], "local");
}

#[test]
fn ops_result_preserves_incomplete_scoped_fixture_fields() {
    let result = parse_ops_result(include_str!("fixtures/ops/incomplete_scoped_result.json"))
        .expect("incomplete scoped result fixture");

    assert_eq!(result.status, "incomplete");
    assert_eq!(result.intake_kind.as_deref(), Some("task"));
    assert_eq!(
        result
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("WP-002A")
    );
    assert_eq!(
        result
            .completion
            .as_ref()
            .map(|completion| completion.missing_evidence.as_slice()),
        Some(["positive scoped completion evidence".to_owned()].as_slice())
    );

    let rendered = render_ops_result(&result);
    assert_eq!(rendered["scoped_work_item"]["source_queue"], Value::Null);
    assert_eq!(
        rendered["completion"]["missing_evidence"],
        json!(["positive scoped completion evidence"])
    );
}

#[test]
fn ops_parser_rejects_legacy_fixture_without_schema() {
    let error = parse_ops_request(include_str!("fixtures/ops/legacy_request.json"))
        .expect_err("legacy request is not an ops request");

    assert!(error.to_string().contains("unsupported ops schema"));
}

#[test]
fn ops_request_rejects_unsupported_schema_and_empty_required_strings() {
    let unsupported = parse_ops_request(
        r#"
        {
          "schema_version": "millracer.ops.v9",
          "request_id": "req-bad",
          "workspace_ref": {},
          "source": {"kind": "cli"},
          "action": "status",
          "input": {}
        }
        "#,
    )
    .expect_err("unsupported schema");
    let empty_action = parse_ops_request(
        r#"
        {
          "schema_version": "millracer.ops.v0.2",
          "request_id": "req-bad",
          "workspace_ref": {},
          "source": {"kind": "cli"},
          "action": "  ",
          "input": {}
        }
        "#,
    )
    .expect_err("empty action");

    assert!(unsupported.to_string().contains("unsupported ops schema"));
    assert!(
        empty_action
            .to_string()
            .contains("ops payload requires non-empty action")
    );
}

#[test]
fn ops_request_uses_python_alias_fallback_for_scoped_work() {
    let request = parse_ops_request(
        r#"
        {
          "schema_version": "millracer.ops.v0.2",
          "request_id": "req-alias",
          "workspace_ref": {},
          "source": {"kind": "cli"},
          "action": "enqueue",
          "scoped_work_item": {},
          "work_item": "bad",
          "scope": {"title": "missing id"},
          "metadata": {"scoped_work_item": {"item_id": "M9"}},
          "input": {"text": "Do scoped work."}
        }
        "#,
    )
    .expect("alias fallback request");

    assert_eq!(
        request
            .scoped_work_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some("M9")
    );
}

#[test]
fn ops_request_trims_optional_strings_and_discards_unusable_collections() {
    let request = parse_ops_request(
        r#"
        {
          "schema_version": "millracer.ops.v0.2",
          "request_id": " req-trim ",
          "workspace_ref": {
            "workspace_id": " ws-1 ",
            "runtime_kind": " ",
            "mode": " learning_codex "
          },
          "source": {"kind": " cli ", "surface": " "},
          "action": " status ",
          "route_preference": " ",
          "intake_preference": null,
          "input": ["not", "an", "object"],
          "options": null,
          "expected_evidence": [" one ", "", 7],
          "context_refs": "bad"
        }
        "#,
    )
    .expect("trimmed request");

    assert_eq!(request.request_id, "req-trim");
    assert_eq!(request.action, "status");
    assert_eq!(request.workspace_ref.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(request.workspace_ref.runtime_kind, "local");
    assert_eq!(
        request.workspace_ref.mode.as_deref(),
        Some("learning_codex")
    );
    assert_eq!(request.source.kind, "cli");
    assert_eq!(request.source.surface, None);
    assert_eq!(request.route_preference, "auto");
    assert_eq!(request.intake_preference, "auto");
    assert!(request.input.is_empty());
    assert!(request.options.is_empty());
    assert_eq!(request.expected_evidence, vec!["one".to_owned()]);
    assert!(request.context_refs.is_empty());

    let rendered = render_ops_request(&request);
    assert!(rendered.get("source").is_some());
    assert!(rendered.get("options").is_none());
    assert!(rendered.get("context_refs").is_none());
}

#[test]
fn ops_result_uses_python_truthiness_and_filters_unusable_nested_items() {
    let result = parse_ops_result(
        r#"
        {
          "schema_version": "millracer.ops.v0.2",
          "request_id": "req-truth",
          "status": "failed",
          "action": "enqueue",
          "workspace_ref": {},
          "started_at": "2026-05-12T00:00:00+00:00",
          "finished_at": "2026-05-12T00:00:01+00:00",
          "warnings": [{"code": "W", "message": "warn", "recoverable": null}],
          "errors": [{"code": "E", "message": "err", "recoverable": "yes"}],
          "result": [],
          "completion": {
            "outcome": "failed",
            "scoped_completion": "true",
            "evidence_summary": [{"kind": "terminal"}, "bad"],
            "missing_evidence": [" proof ", "", 1],
            "verification_status": ""
          }
        }
        "#,
    )
    .expect("truthy result");

    assert!(!result.warnings[0].recoverable);
    assert!(result.errors[0].recoverable);
    assert!(result.result.is_empty());
    assert_eq!(
        result.completion,
        Some(Completion {
            outcome: "failed".to_owned(),
            scoped_completion: true,
            evidence_summary: vec![Map::from_iter([(
                "kind".to_owned(),
                Value::String("terminal".to_owned())
            )])],
            missing_evidence: vec!["proof".to_owned()],
            verification_status: "unverified".to_owned(),
            terminal_outcome_ref: None,
        })
    );
}

#[test]
fn ops_event_frame_renders_sequence_payload_and_cursor() {
    let frame = OpsEventFrame {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: "req-001".to_owned(),
        event_id: "evt-001".to_owned(),
        sequence: 1,
        timestamp: "2026-05-12T00:00:00+00:00".to_owned(),
        event_type: "request.accepted".to_owned(),
        severity: "info".to_owned(),
        message: "Request accepted.".to_owned(),
        payload: Map::from_iter([("accepted".to_owned(), Value::Bool(true))]),
        cursor: Some("1".to_owned()),
    };

    let rendered = render_ops_event_frame(&frame);

    assert_eq!(rendered["event_type"], "request.accepted");
    assert_eq!(rendered["sequence"], 1);
    assert_eq!(rendered["payload"]["accepted"], true);
    assert_eq!(rendered["cursor"], "1");
}

#[test]
fn warning_error_and_completion_records_are_public_model_types() {
    let warning = WarningRecord {
        code: "warn".to_owned(),
        message: "Heads up.".to_owned(),
        severity: "warning".to_owned(),
        recoverable: true,
        related_ref: None,
        suggested_action: None,
    };
    let error = ErrorRecord {
        code: "failed".to_owned(),
        message: "Failed.".to_owned(),
        severity: "error".to_owned(),
        recoverable: false,
        related_ref: Some("req-1".to_owned()),
        suggested_action: Some("retry".to_owned()),
    };

    assert!(warning.recoverable);
    assert_eq!(error.related_ref.as_deref(), Some("req-1"));
}
