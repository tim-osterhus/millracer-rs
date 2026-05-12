use millracer::prompts::{
    FinalizationPrompt, MILLRACER_SYSTEM_PROMPT, decision_prompt, finalization_prompt,
};

const FORBIDDEN_PROMPT_TERMS: &[&str] = &[
    "EvoClaw",
    "hidden eval",
    "hidden evaluation",
    "milestone",
    "scoreboard",
    "benchmark",
];

#[test]
fn system_prompt_explains_generic_intake_kind_selection() {
    let prompt = MILLRACER_SYSTEM_PROMPT.to_ascii_lowercase();

    assert!(prompt.contains("route and intake kind"));
    assert!(prompt.contains("large pre-existing codebase"));
    assert!(prompt.contains("probe when in doubt"));
    assert!(prompt.contains("probe means investigate before implementation"));
    assert!(prompt.contains("task is only for tightly scoped execution-ready work"));
    assert!(prompt.contains("idea is for clear outcomes"));
}

#[test]
fn decision_prompt_requests_intake_kind_and_generic_signals() {
    let prompt = decision_prompt("Change runtime behavior");

    assert!(prompt.contains(r#""intake_kind": "probe|idea|task""#));
    assert!(prompt.contains(r#""signals": ["<short generic signal>", "..."]"#));
    assert!(prompt.contains("In a large pre-existing codebase, choose probe when in doubt."));
}

#[test]
fn finalization_prompt_reports_intake_kind_and_scope() {
    let prompt = finalization_prompt(FinalizationPrompt {
        task: "Do one scoped item.",
        workspace: "/tmp/ws",
        route: "millrace",
        intake_kind: "probe",
        event_kind: "complete",
        event_reason: "done",
        status_json: "{}",
        warnings: &[],
        scoped_work_json: Some(r#"{"item_id": "ITEM-123"}"#),
        progress_events_json: None,
    });

    assert!(prompt.contains("- route: millrace"));
    assert!(prompt.contains("- intake kind: probe"));
    assert!(prompt.contains(r#"{"item_id": "ITEM-123"}"#));
}

#[test]
fn injected_prompts_avoid_forbidden_benchmark_specific_terms() {
    let prompts = [
        MILLRACER_SYSTEM_PROMPT.to_owned(),
        decision_prompt("Change runtime behavior"),
        finalization_prompt(FinalizationPrompt {
            task: "Do one scoped item.",
            workspace: "/tmp/ws",
            route: "millrace",
            intake_kind: "probe",
            event_kind: "complete",
            event_reason: "done",
            status_json: "{}",
            warnings: &[],
            scoped_work_json: None,
            progress_events_json: None,
        }),
    ];

    for prompt in prompts {
        let prompt = prompt.to_ascii_lowercase();
        for term in FORBIDDEN_PROMPT_TERMS {
            assert!(!prompt.contains(&term.to_ascii_lowercase()));
        }
    }
}
