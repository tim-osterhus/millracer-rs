use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use millracer::MillracerResult;
use millracer::sessions::SessionStore;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "millracer-sessions-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn session_store_remembers_selected_workspace() -> MillracerResult<()> {
    let store = SessionStore::new(temp_dir("selection").join("sessions.json"));
    let session = store.get_or_create("local")?;

    store.update_selection(
        &session.session_id,
        Some("millrace-os".to_owned()),
        Some("learning_codex".to_owned()),
    )?;
    let loaded = store.get_or_create("local")?;

    assert_eq!(loaded.selected_workspace_id.as_deref(), Some("millrace-os"));
    assert_eq!(loaded.selected_mode.as_deref(), Some("learning_codex"));
    Ok(())
}

#[test]
fn session_store_tracks_recent_request_ids_without_runtime_truth() -> MillracerResult<()> {
    let path = temp_dir("request-records").join("sessions.json");
    let store = SessionStore::new(&path);
    let session = store.get_or_create("mission-control")?;

    store.record_request(
        &session.session_id,
        "req-001",
        vec!["workspace_stale".to_owned()],
    )?;
    let loaded = store.get_or_create("mission-control")?;
    let raw = fs::read_to_string(&path)?;

    assert_eq!(loaded.recent_request_ids, vec!["req-001".to_owned()]);
    assert_eq!(
        loaded.last_warning_codes,
        vec!["workspace_stale".to_owned()]
    );
    for forbidden in [
        "queue_depth",
        "terminal_outcome",
        "trace",
        "approval",
        "artifact",
        "work_item_lifecycle",
    ] {
        assert!(
            !raw.contains(forbidden),
            "stored runtime truth field {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn session_store_keeps_recent_request_ids_bounded() -> MillracerResult<()> {
    let store =
        SessionStore::with_max_recent_requests(temp_dir("bounded").join("sessions.json"), 2);
    let session = store.get_or_create("local")?;

    store.record_request(&session.session_id, "req-001", Vec::new())?;
    store.record_request(&session.session_id, "req-002", Vec::new())?;
    store.record_request(&session.session_id, "req-003", Vec::new())?;
    let loaded = store.get_or_create("local")?;

    assert_eq!(
        loaded.recent_request_ids,
        vec!["req-002".to_owned(), "req-003".to_owned()]
    );
    Ok(())
}

#[test]
fn session_store_records_last_warning_codes() -> MillracerResult<()> {
    let store = SessionStore::new(temp_dir("warnings").join("sessions.json"));
    let session = store.get_or_create("local")?;

    store.record_request(
        &session.session_id,
        "req-001",
        vec!["workspace_stale".to_owned(), "mode_defaulted".to_owned()],
    )?;
    let loaded = store.get_or_create("local")?;

    assert_eq!(
        loaded.last_warning_codes,
        vec!["workspace_stale".to_owned(), "mode_defaulted".to_owned()]
    );
    Ok(())
}

#[test]
fn session_store_lists_sessions_sorted_by_session_id() -> MillracerResult<()> {
    let store = SessionStore::new(temp_dir("listing").join("sessions.json"));
    store.get_or_create("mission-control")?;
    store.get_or_create("local")?;

    let sessions = store.list_sessions()?;

    assert_eq!(
        sessions
            .iter()
            .map(|session| session.client_name.as_str())
            .collect::<Vec<_>>(),
        vec!["local", "mission-control"]
    );
    Ok(())
}

#[test]
fn session_store_ignores_unusable_stored_session_fields() -> MillracerResult<()> {
    let path = temp_dir("load-filter").join("sessions.json");
    fs::write(
        &path,
        r#"
        {
          "sessions": {
            "local": {
              "client_name": " local ",
              "selected_workspace_id": " ws-1 ",
              "selected_mode": "",
              "recent_request_ids": [" req-001 ", "", 7],
              "last_warning_codes": "bad"
            },
            "bad": "not an object"
          }
        }
        "#,
    )?;
    let store = SessionStore::new(path);

    let session = store.get_or_create("local")?;

    assert_eq!(session.client_name, "local");
    assert_eq!(session.selected_workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(session.selected_mode, None);
    assert_eq!(session.recent_request_ids, vec!["req-001".to_owned()]);
    assert!(session.last_warning_codes.is_empty());
    Ok(())
}
