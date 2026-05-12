use millracer::scope::{ScopedWorkItem, render_scoped_work, scoped_work_json};
use serde_json::json;

#[test]
fn scoped_work_item_accepts_python_aliases_and_constraints() {
    let scoped = ScopedWorkItem::from_payload(Some(&json!({
        "id": "M06",
        "title": "Array API label encoder support",
        "source_queue": "/queue.md",
        "spec_path": "/srs/M06.md",
        "completion_ref": "agent-impl-M06",
        "constraints": ["Do not implement any other queue item.", "", 5]
    })))
    .expect("scoped payload");

    assert_eq!(scoped.item_id, "M06");
    assert_eq!(
        scoped.title.as_deref(),
        Some("Array API label encoder support")
    );
    assert_eq!(
        scoped.constraints,
        vec!["Do not implement any other queue item.".to_owned()]
    );
}

#[test]
fn render_scoped_work_carries_constraints_into_intake_documents() {
    let scoped = ScopedWorkItem {
        item_id: "M06".to_owned(),
        title: Some("Array API label encoder support".to_owned()),
        source_queue: Some("/queue.md".to_owned()),
        spec_path: Some("/srs/M06.md".to_owned()),
        completion_ref: Some("agent-impl-M06".to_owned()),
        constraints: vec!["Do not implement any other queue item.".to_owned()],
    };

    let raw = render_scoped_work(Some(&scoped));

    assert!(raw.contains("Scoped-Work:"));
    assert!(raw.contains("Item-ID: M06"));
    assert!(raw.contains("Spec-Path: /srs/M06.md"));
    assert!(raw.contains("Completion-Ref: agent-impl-M06"));
    assert!(raw.contains("Constraint: Do not implement any other queue item."));
}

#[test]
fn scoped_work_json_is_stable_jsonable_metadata() {
    let scoped = ScopedWorkItem {
        item_id: "ITEM-123".to_owned(),
        title: None,
        source_queue: None,
        spec_path: None,
        completion_ref: None,
        constraints: Vec::new(),
    };

    let raw = scoped_work_json(Some(&scoped)).expect("json");

    assert!(raw.contains(r#""item_id": "ITEM-123""#));
    assert!(raw.contains(r#""constraints": []"#));
}
