use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: String,
    pub client_name: String,
    pub selected_workspace_id: Option<String>,
    pub selected_mode: Option<String>,
    pub recent_request_ids: Vec<String>,
    pub last_warning_codes: Vec<String>,
}

impl SessionRecord {
    pub fn new(session_id: impl Into<String>, client_name: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            client_name: client_name.into(),
            selected_workspace_id: None,
            selected_mode: None,
            recent_request_ids: Vec::new(),
            last_warning_codes: Vec::new(),
        }
    }

    pub fn to_jsonable(&self) -> Value {
        let mut payload = Map::new();
        payload.insert(
            "session_id".to_owned(),
            Value::String(self.session_id.clone()),
        );
        payload.insert(
            "client_name".to_owned(),
            Value::String(self.client_name.clone()),
        );
        payload.insert(
            "selected_workspace_id".to_owned(),
            option_string_value(&self.selected_workspace_id),
        );
        payload.insert(
            "selected_mode".to_owned(),
            option_string_value(&self.selected_mode),
        );
        payload.insert(
            "recent_request_ids".to_owned(),
            string_array(&self.recent_request_ids),
        );
        payload.insert(
            "last_warning_codes".to_owned(),
            string_array(&self.last_warning_codes),
        );
        Value::Object(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStore {
    pub path: PathBuf,
    pub max_recent_requests: usize,
}

impl SessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_recent_requests: 20,
        }
    }

    pub fn with_max_recent_requests(path: impl Into<PathBuf>, max_recent_requests: usize) -> Self {
        Self {
            path: path.into(),
            max_recent_requests,
        }
    }

    pub fn get_or_create(&self, client_name: &str) -> MillracerResult<SessionRecord> {
        let mut sessions = self.load()?;
        if let Some(session) = sessions
            .values()
            .find(|session| session.client_name == client_name)
            .cloned()
        {
            return Ok(session);
        }

        let session = SessionRecord::new(client_name, client_name);
        sessions.insert(session.session_id.clone(), session.clone());
        self.save(&sessions)?;
        Ok(session)
    }

    pub fn update_selection(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
        mode: Option<String>,
    ) -> MillracerResult<SessionRecord> {
        let mut sessions = self.load()?;
        let session = sessions
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| SessionRecord::new(session_id, session_id));
        let updated = SessionRecord {
            session_id: session.session_id,
            client_name: session.client_name,
            selected_workspace_id: workspace_id,
            selected_mode: mode,
            recent_request_ids: session.recent_request_ids,
            last_warning_codes: session.last_warning_codes,
        };
        sessions.insert(session_id.to_owned(), updated.clone());
        self.save(&sessions)?;
        Ok(updated)
    }

    pub fn record_request(
        &self,
        session_id: &str,
        request_id: &str,
        warning_codes: Vec<String>,
    ) -> MillracerResult<SessionRecord> {
        let mut sessions = self.load()?;
        let session = sessions
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| SessionRecord::new(session_id, session_id));
        let mut recent_request_ids = session.recent_request_ids;
        recent_request_ids.push(request_id.to_owned());
        if self.max_recent_requests > 0 && recent_request_ids.len() > self.max_recent_requests {
            let keep_from = recent_request_ids.len() - self.max_recent_requests;
            recent_request_ids = recent_request_ids.split_off(keep_from);
        }

        let updated = SessionRecord {
            session_id: session.session_id,
            client_name: session.client_name,
            selected_workspace_id: session.selected_workspace_id,
            selected_mode: session.selected_mode,
            recent_request_ids,
            last_warning_codes: warning_codes,
        };
        sessions.insert(session_id.to_owned(), updated.clone());
        self.save(&sessions)?;
        Ok(updated)
    }

    pub fn list_sessions(&self) -> MillracerResult<Vec<SessionRecord>> {
        Ok(self.load()?.into_values().collect())
    }

    fn load(&self) -> MillracerResult<BTreeMap<String, SessionRecord>> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let raw = fs::read_to_string(&self.path)?;
        let payload: Value = serde_json::from_str(if raw.trim().is_empty() { "{}" } else { &raw })?;
        let Some(payload) = payload.as_object() else {
            return Ok(BTreeMap::new());
        };
        let Some(sessions_payload) = payload.get("sessions").and_then(Value::as_object) else {
            return Ok(BTreeMap::new());
        };

        let mut sessions = BTreeMap::new();
        for (session_id, raw_session) in sessions_payload {
            let Some(raw_session) = raw_session.as_object() else {
                continue;
            };
            sessions.insert(
                session_id.clone(),
                SessionRecord {
                    session_id: session_id.clone(),
                    client_name: string_or(raw_session.get("client_name"), session_id),
                    selected_workspace_id: optional_string(
                        raw_session.get("selected_workspace_id"),
                    ),
                    selected_mode: optional_string(raw_session.get("selected_mode")),
                    recent_request_ids: string_list(raw_session.get("recent_request_ids")),
                    last_warning_codes: string_list(raw_session.get("last_warning_codes")),
                },
            );
        }
        Ok(sessions)
    }

    fn save(&self, sessions: &BTreeMap<String, SessionRecord>) -> MillracerResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut sessions_payload = Map::new();
        for (session_id, session) in sessions {
            sessions_payload.insert(session_id.clone(), session.to_jsonable());
        }

        let mut payload = Map::new();
        payload.insert("sessions".to_owned(), Value::Object(sessions_payload));
        fs::write(
            &self.path,
            serde_json::to_string_pretty(&Value::Object(payload)).map_err(MillracerError::from)?,
        )?;
        Ok(())
    }
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn string_or(value: Option<&Value>, default: &str) -> String {
    optional_string(value).unwrap_or_else(|| default.to_owned())
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| optional_string(Some(item)))
        .collect()
}

fn option_string_value(value: &Option<String>) -> Value {
    value
        .as_ref()
        .map(|value| Value::String(value.clone()))
        .unwrap_or(Value::Null)
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::String(value.clone()))
            .collect(),
    )
}
