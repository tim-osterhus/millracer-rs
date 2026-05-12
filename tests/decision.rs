use millracer::decision::{Decision, parse_decision};

#[test]
fn parse_decision_reads_fenced_json() {
    let raw = r#"
    Here is the decision:

    ```json
    {
      "decision": "millrace",
      "intake_kind": "probe",
      "why": "multi-stage",
      "mode": "learning_pi",
      "signals": ["large pre-existing codebase", "uncertain affected files"]
    }
    ```
    "#;

    assert_eq!(
        parse_decision(raw),
        Decision {
            route: "millrace".to_owned(),
            why: "multi-stage".to_owned(),
            mode: "learning_pi".to_owned(),
            custom_loop_needed: false,
            notes: String::new(),
            intake_kind: Some("probe".to_owned()),
            signals: vec![
                "large pre-existing codebase".to_owned(),
                "uncertain affected files".to_owned()
            ],
        }
    );
}

#[test]
fn parse_decision_preserves_custom_loop_signal() {
    let decision = parse_decision(
        r#"{"decision": "millrace", "why": "needs a special topology", "custom_loop_needed": true, "notes": "use a custom loop"}"#,
    );

    assert!(decision.custom_loop_needed);
    assert_eq!(decision.notes, "use a custom loop");
}

#[test]
fn parse_decision_falls_back_to_route_lines() {
    let decision = parse_decision("decision: direct\nwhy: small edit");

    assert_eq!(decision.route, "direct");
    assert_eq!(decision.why, "small edit");
    assert_eq!(decision.mode, "default_pi");
}

#[test]
fn parse_decision_normalizes_invalid_intake_to_none() {
    let decision = parse_decision(
        r#"{"decision": "millrace", "intake_kind": "whatever", "why": "multi-stage"}"#,
    );

    assert_eq!(decision.intake_kind, None);
}

#[test]
fn parse_decision_defaults_unstructured_output_to_direct() {
    let decision = parse_decision("I would probably do this locally.");

    assert_eq!(decision.route, "direct");
    assert_eq!(decision.why, "Pi returned unstructured output.");
}
