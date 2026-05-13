use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use millracer::ops_models::{JsonObject, OpsRequest, SCHEMA_VERSION, SourceRef, WorkspaceRef};
use millracer::workspaces::{WorkspaceRecord, WorkspaceRegistry, resolve_workspace};

fn request(workspace_ref: WorkspaceRef) -> OpsRequest {
    OpsRequest {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: "req-001".to_owned(),
        action: "status".to_owned(),
        workspace_ref,
        source: SourceRef {
            kind: "cli".to_owned(),
            surface: None,
            adapter_id: None,
            conversation_id: None,
            message_id: None,
            parent_request_id: None,
            trace_ref: None,
        },
        input: JsonObject::new(),
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

fn workspace_ref() -> WorkspaceRef {
    WorkspaceRef {
        workspace_id: None,
        root_path: None,
        display_name: None,
        runtime_kind: "local".to_owned(),
        mode: None,
        environment: None,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "millracer-workspaces-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn record(workspace_id: &str, root_path: &Path) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: workspace_id.to_owned(),
        root_path: root_path.to_path_buf(),
        display_name: None,
        default_mode: None,
        tags: Vec::new(),
    }
}

#[test]
fn workspace_resolution_prefers_request_root_path() {
    let root = temp_dir("request-root");
    let mut workspace_ref = workspace_ref();
    workspace_ref.root_path = Some(root.to_string_lossy().into_owned());
    workspace_ref.display_name = Some("Local workspace".to_owned());
    workspace_ref.mode = Some("learning_codex".to_owned());

    let result = resolve_workspace(
        &request(workspace_ref),
        &WorkspaceRegistry::empty(),
        None,
        None,
        Some(Path::new(".")),
    );

    assert_eq!(result.root_path.as_deref(), Some(root.as_path()));
    assert_eq!(result.strategy, "request_root_path");
    assert!(result.validated);
    assert_eq!(result.mode.as_deref(), Some("learning_codex"));
    assert_eq!(result.display_name.as_deref(), Some("Local workspace"));
}

#[test]
fn workspace_resolution_prefers_request_workspace_id() {
    let root = temp_dir("request-id");
    let mut record = record("millrace-os", &root);
    record.display_name = Some("Millrace OS".to_owned());
    record.default_mode = Some("learning_codex".to_owned());
    let registry = WorkspaceRegistry {
        records: BTreeMap::from([("millrace-os".to_owned(), record)]),
        default_workspace_id: None,
    };
    let mut workspace_ref = workspace_ref();
    workspace_ref.workspace_id = Some("millrace-os".to_owned());

    let result = resolve_workspace(&request(workspace_ref), &registry, None, None, None);

    assert_eq!(result.root_path.as_deref(), Some(root.as_path()));
    assert_eq!(result.workspace_id.as_deref(), Some("millrace-os"));
    assert_eq!(result.strategy, "request_workspace_id");
    assert_eq!(result.mode.as_deref(), Some("learning_codex"));
    assert_eq!(result.display_name.as_deref(), Some("Millrace OS"));
}

#[test]
fn workspace_resolution_falls_back_to_request_root_when_id_is_unknown() {
    let root = temp_dir("unknown-id-root");
    let mut workspace_ref = workspace_ref();
    workspace_ref.workspace_id = Some("unknown".to_owned());
    workspace_ref.root_path = Some(root.to_string_lossy().into_owned());
    workspace_ref.mode = Some("learning_codex".to_owned());

    let result = resolve_workspace(
        &request(workspace_ref),
        &WorkspaceRegistry::empty(),
        None,
        None,
        None,
    );

    assert_eq!(result.root_path.as_deref(), Some(root.as_path()));
    assert_eq!(result.workspace_id.as_deref(), Some("unknown"));
    assert_eq!(result.strategy, "request_root_path");
    assert_eq!(result.mode.as_deref(), Some("learning_codex"));
}

#[test]
fn workspace_resolution_reports_unknown_id_without_guessing() {
    let mut workspace_ref = workspace_ref();
    workspace_ref.workspace_id = Some("unknown".to_owned());

    let result = resolve_workspace(
        &request(workspace_ref),
        &WorkspaceRegistry::empty(),
        None,
        None,
        Some(Path::new(".")),
    );

    assert_eq!(result.root_path, None);
    assert_eq!(result.strategy, "request_workspace_id");
    assert_eq!(result.error_code.as_deref(), Some("workspace_unresolved"));
}

#[test]
fn workspace_resolution_uses_cli_default_before_registry_default() {
    let registry_root = temp_dir("registry-default");
    let cli_root = temp_dir("cli-default");
    let registry = WorkspaceRegistry {
        records: BTreeMap::from([("default".to_owned(), record("default", &registry_root))]),
        default_workspace_id: Some("default".to_owned()),
    };

    let result = resolve_workspace(
        &request(workspace_ref()),
        &registry,
        Some(&cli_root),
        None,
        None,
    );

    assert_eq!(result.root_path.as_deref(), Some(cli_root.as_path()));
    assert_eq!(result.strategy, "cli_workspace");
}

#[test]
fn workspace_resolution_uses_active_session_before_registry_default() {
    let active_root = temp_dir("active");
    let default_root = temp_dir("default");
    let mut active = record("active", &active_root);
    active.default_mode = Some("active_mode".to_owned());
    let mut default = record("default", &default_root);
    default.default_mode = Some("default_mode".to_owned());
    let registry = WorkspaceRegistry {
        records: BTreeMap::from([
            ("active".to_owned(), active),
            ("default".to_owned(), default),
        ]),
        default_workspace_id: Some("default".to_owned()),
    };

    let result = resolve_workspace(
        &request(workspace_ref()),
        &registry,
        None,
        Some("active"),
        None,
    );

    assert_eq!(result.root_path.as_deref(), Some(active_root.as_path()));
    assert_eq!(result.workspace_id.as_deref(), Some("active"));
    assert_eq!(result.strategy, "active_session");
    assert_eq!(result.mode.as_deref(), Some("active_mode"));
}

#[test]
fn workspace_resolution_uses_registry_default_after_active_session_miss() {
    let root = temp_dir("registry-fallback");
    let registry = WorkspaceRegistry {
        records: BTreeMap::from([("default".to_owned(), record("default", &root))]),
        default_workspace_id: Some("default".to_owned()),
    };

    let result = resolve_workspace(
        &request(workspace_ref()),
        &registry,
        None,
        Some("missing"),
        None,
    );

    assert_eq!(result.root_path.as_deref(), Some(root.as_path()));
    assert_eq!(result.workspace_id.as_deref(), Some("default"));
    assert_eq!(result.strategy, "registry_default");
}

#[test]
fn workspace_resolution_uses_cwd_only_when_it_exists() {
    let cwd = temp_dir("cwd");
    let missing = cwd.join("missing");

    let found = resolve_workspace(
        &request(workspace_ref()),
        &WorkspaceRegistry::empty(),
        None,
        None,
        Some(&cwd),
    );
    let unresolved = resolve_workspace(
        &request(workspace_ref()),
        &WorkspaceRegistry::empty(),
        None,
        None,
        Some(&missing),
    );

    assert_eq!(found.root_path.as_deref(), Some(cwd.as_path()));
    assert_eq!(found.strategy, "cwd");
    assert!(found.validated);
    assert_eq!(unresolved.root_path, None);
    assert_eq!(
        unresolved.error_code.as_deref(),
        Some("workspace_unresolved")
    );
}

#[test]
fn workspace_resolution_reports_unresolved_without_guessing() {
    let result = resolve_workspace(
        &request(workspace_ref()),
        &WorkspaceRegistry::empty(),
        None,
        None,
        None,
    );

    assert_eq!(result.root_path, None);
    assert_eq!(result.strategy, "unresolved");
    assert_eq!(result.error_code.as_deref(), Some("workspace_unresolved"));
}
