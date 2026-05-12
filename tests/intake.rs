use millracer::decision::Decision;
use millracer::intake::{IntakeKind, choose_intake_kind};

#[test]
fn large_preexisting_codebase_with_uncertain_surface_defaults_to_probe() {
    let decision = choose_intake_kind(
        "Update behavior in this large pre-existing codebase; affected files are uncertain.",
        IntakeKind::Auto,
        None,
    );

    assert_eq!(decision.intake_kind, IntakeKind::Probe);
    assert!(
        decision
            .signals
            .contains(&"large pre-existing codebase".to_owned())
    );
}

#[test]
fn broad_migration_defaults_to_probe() {
    let decision = choose_intake_kind(
        "Migrate the runtime configuration architecture across the repo.",
        IntakeKind::Auto,
        None,
    );

    assert_eq!(decision.intake_kind, IntakeKind::Probe);
}

#[test]
fn understand_before_changing_defaults_to_probe() {
    let decision = choose_intake_kind(
        "Need to understand the codebase before changing how the harness launches.",
        IntakeKind::Auto,
        None,
    );

    assert_eq!(decision.intake_kind, IntakeKind::Probe);
}

#[test]
fn clear_product_capability_defaults_to_idea() {
    let decision = choose_intake_kind(
        "Build a new operator dashboard capability for viewing queued runs.",
        IntakeKind::Auto,
        None,
    );

    assert_eq!(decision.intake_kind, IntakeKind::Idea);
}

#[test]
fn exact_file_and_failing_test_defaults_to_task() {
    let decision = choose_intake_kind(
        "Fix src/millracer/monitor.py so tests/test_monitor.py::test_idle passes.",
        IntakeKind::Auto,
        None,
    );

    assert_eq!(decision.intake_kind, IntakeKind::Task);
}

#[test]
fn forced_intake_overrides_auto_classification() {
    let task = "Update behavior in a large pre-existing codebase with uncertain affected files.";

    assert_eq!(
        choose_intake_kind(task, IntakeKind::Task, None).intake_kind,
        IntakeKind::Task
    );
    assert_eq!(
        choose_intake_kind(task, IntakeKind::Idea, None).intake_kind,
        IntakeKind::Idea
    );
    assert_eq!(
        choose_intake_kind(task, IntakeKind::Probe, None).intake_kind,
        IntakeKind::Probe
    );
}

#[test]
fn decision_intake_is_used_when_auto_requested() {
    let mut decision = Decision::new("millrace", "needs planning");
    decision.intake_kind = Some("idea".to_owned());
    decision.signals = vec!["clear outcome needing shaping".to_owned()];

    let chosen = choose_intake_kind(
        "Create a new user-facing workflow.",
        IntakeKind::Auto,
        Some(&decision),
    );

    assert_eq!(chosen.intake_kind, IntakeKind::Idea);
    assert_eq!(
        chosen.signals,
        vec!["clear outcome needing shaping".to_owned()]
    );
}

#[test]
fn malformed_or_missing_intake_falls_back_to_conservative_probe() {
    let decision = choose_intake_kind(
        "Adjust the runtime based on the attached issue.",
        IntakeKind::Auto,
        Some(&Decision::new("millrace", "unstructured")),
    );

    assert_eq!(decision.intake_kind, IntakeKind::Probe);
}
