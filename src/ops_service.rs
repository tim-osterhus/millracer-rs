use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Number, Value};

use crate::MillracerResult;
use crate::agent::{AgentSession, MillraceLike, RunOptions};
use crate::benchmark::{RunResult, render_benchmark_result};
use crate::ops_models::{
    Completion, ErrorRecord, JsonObject, OpsEventFrame, OpsRequest, OpsResult, SCHEMA_VERSION,
    WarningRecord, WorkspaceRef,
};
use crate::sessions::SessionStore;
use crate::workspaces::{WorkspaceRegistry, WorkspaceResolution, resolve_workspace};

pub trait OpsRuntime {
    fn status(&mut self) -> MillracerResult<JsonObject>;
}

impl<T> OpsRuntime for T
where
    T: MillraceLike,
{
    fn status(&mut self) -> MillracerResult<JsonObject> {
        MillraceLike::status(self)
    }
}

pub type RuntimeFactory = Box<dyn FnMut(&WorkspaceResolution) -> Box<dyn OpsRuntime>>;
pub type AgentFactory =
    Box<dyn FnMut(&WorkspaceResolution, Box<dyn OpsRuntime>) -> Box<dyn AgentSession>>;

pub struct OpsService {
    pub runtime_factory: RuntimeFactory,
    pub agent_factory: Option<AgentFactory>,
    pub registry: WorkspaceRegistry,
    pub session_store: Option<SessionStore>,
    pub cli_workspace: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
}

impl OpsService {
    pub fn new<F, R>(mut runtime_factory: F) -> Self
    where
        F: FnMut(&WorkspaceResolution) -> R + 'static,
        R: OpsRuntime + 'static,
    {
        Self {
            runtime_factory: Box::new(move |resolution| Box::new(runtime_factory(resolution))),
            agent_factory: None,
            registry: WorkspaceRegistry::empty(),
            session_store: None,
            cli_workspace: None,
            cwd: Some(PathBuf::from(".")),
        }
    }

    pub fn with_agent_factory<F, A>(mut self, mut agent_factory: F) -> Self
    where
        F: FnMut(&WorkspaceResolution, Box<dyn OpsRuntime>) -> A + 'static,
        A: AgentSession + 'static,
    {
        self.agent_factory = Some(Box::new(move |resolution, runtime| {
            Box::new(agent_factory(resolution, runtime))
        }));
        self
    }

    pub fn handle(&mut self, request: &OpsRequest) -> OpsResult {
        let started_at = now();
        match request.action.as_str() {
            "list_workspaces" => return self.list_workspaces_result(request, started_at),
            "list_sessions" => return self.list_sessions_result(request, started_at),
            "select_workspace" => return self.select_workspace_result(request, started_at),
            "inspect_session" => return self.inspect_session_result(request, started_at),
            _ => {}
        }

        let resolution = resolve_workspace(
            request,
            &self.registry,
            self.cli_workspace.as_deref(),
            None,
            self.cwd.as_deref(),
        );
        if resolution.error_code.is_some() || resolution.root_path.is_none() {
            return self.error_result(
                request,
                started_at,
                &resolution,
                ErrorRecord {
                    code: resolution
                        .error_code
                        .clone()
                        .unwrap_or_else(|| "workspace_unresolved".to_owned()),
                    message: "Unable to resolve Millrace workspace.".to_owned(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: Some(
                        "Pass workspace_ref.root_path or register/select a workspace.".to_owned(),
                    ),
                },
            );
        }

        match request.action.as_str() {
            "status" => self.status_result(request, started_at, &resolution),
            "enqueue" => self.enqueue_result(request, started_at, &resolution),
            _ => self.error_result(
                request,
                started_at,
                &resolution,
                ErrorRecord {
                    code: "unsupported_action".to_owned(),
                    message: format!("Unsupported Millracer ops action: {}", request.action),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: Some(
                        "Use status or enqueue in this Millracer version.".to_owned(),
                    ),
                },
            ),
        }
    }

    pub fn preview_events(&self, request: &OpsRequest) -> Vec<OpsEventFrame> {
        vec![OpsEventFrame {
            schema_version: SCHEMA_VERSION.to_owned(),
            request_id: request.request_id.clone(),
            event_id: format!("{}-accepted", request.request_id),
            sequence: 1,
            timestamp: now(),
            event_type: "request.accepted".to_owned(),
            severity: "info".to_owned(),
            message: "Request accepted.".to_owned(),
            payload: JsonObject::from_iter([(
                "action".to_owned(),
                Value::String(request.action.clone()),
            )]),
            cursor: Some("1".to_owned()),
        }]
    }

    fn list_workspaces_result(&self, request: &OpsRequest, started_at: String) -> OpsResult {
        let workspaces = self
            .registry
            .records
            .iter()
            .map(|(workspace_id, record)| {
                Value::Object(JsonObject::from_iter([
                    (
                        "workspace_id".to_owned(),
                        Value::String(record.workspace_id.clone()),
                    ),
                    (
                        "root_path".to_owned(),
                        Value::String(record.root_path.display().to_string()),
                    ),
                    (
                        "display_name".to_owned(),
                        option_string_value(&record.display_name),
                    ),
                    (
                        "default_mode".to_owned(),
                        option_string_value(&record.default_mode),
                    ),
                    ("tags".to_owned(), string_array(&record.tags)),
                    (
                        "is_default".to_owned(),
                        Value::Bool(
                            self.registry.default_workspace_id.as_deref()
                                == Some(workspace_id.as_str()),
                        ),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        self.success_result(
            request,
            started_at,
            JsonObject::from_iter([("workspaces".to_owned(), Value::Array(workspaces))]),
            "inspect_only",
            None,
        )
    }

    fn list_sessions_result(&self, request: &OpsRequest, started_at: String) -> OpsResult {
        let sessions = match self.session_store.as_ref() {
            Some(store) => match store.list_sessions() {
                Ok(sessions) => sessions,
                Err(error) => {
                    return self.session_store_error_result(request, started_at, error.to_string());
                }
            },
            None => Vec::new(),
        }
        .into_iter()
        .map(|session| session.to_jsonable())
        .collect::<Vec<_>>();
        self.success_result(
            request,
            started_at,
            JsonObject::from_iter([("sessions".to_owned(), Value::Array(sessions))]),
            "inspect_only",
            None,
        )
    }

    fn select_workspace_result(&self, request: &OpsRequest, started_at: String) -> OpsResult {
        let Some(session_store) = self.session_store.as_ref() else {
            return self.error_result(
                request,
                started_at,
                &empty_resolution(),
                ErrorRecord {
                    code: "session_store_unavailable".to_owned(),
                    message: "select_workspace requires a session store.".to_owned(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            );
        };

        let payload = payload(&request.input);
        let workspace_id = optional_string(payload.get("workspace_id"));
        let session_id =
            optional_string(payload.get("session_id")).unwrap_or_else(|| "local".to_owned());
        let Some(workspace_id) = workspace_id else {
            return self.workspace_selection_error(request, started_at);
        };
        let Some(record) = self.registry.get(Some(&workspace_id)) else {
            return self.workspace_selection_error(request, started_at);
        };
        match session_store.update_selection(
            &session_id,
            Some(workspace_id.clone()),
            record.default_mode.clone(),
        ) {
            Ok(session) => self.success_result(
                request,
                started_at,
                JsonObject::from_iter([("session".to_owned(), session.to_jsonable())]),
                "inspect_only",
                Some(session.session_id),
            ),
            Err(error) => self.error_result(
                request,
                started_at,
                &empty_resolution(),
                ErrorRecord {
                    code: "runtime_command_failed".to_owned(),
                    message: error.to_string(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            ),
        }
    }

    fn workspace_selection_error(&self, request: &OpsRequest, started_at: String) -> OpsResult {
        self.error_result(
            request,
            started_at,
            &empty_resolution(),
            ErrorRecord {
                code: "workspace_unresolved".to_owned(),
                message: "select_workspace requires a registered workspace_id.".to_owned(),
                severity: "error".to_owned(),
                recoverable: true,
                related_ref: None,
                suggested_action: None,
            },
        )
    }

    fn inspect_session_result(&self, request: &OpsRequest, started_at: String) -> OpsResult {
        let sessions = match self.session_store.as_ref() {
            Some(store) => match store.list_sessions() {
                Ok(sessions) => sessions,
                Err(error) => {
                    return self.session_store_error_result(request, started_at, error.to_string());
                }
            },
            None => Vec::new(),
        };
        let payload = payload(&request.input);
        let session_id = optional_string(payload.get("session_id"));
        let selected = session_id.as_ref().and_then(|session_id| {
            sessions
                .iter()
                .find(|session| session.session_id == *session_id)
                .map(|session| session.to_jsonable())
        });
        self.success_result(
            request,
            started_at,
            JsonObject::from_iter([("session".to_owned(), selected.unwrap_or(Value::Null))]),
            "inspect_only",
            session_id,
        )
    }

    fn session_store_error_result(
        &self,
        request: &OpsRequest,
        started_at: String,
        message: String,
    ) -> OpsResult {
        self.error_result(
            request,
            started_at,
            &empty_resolution(),
            ErrorRecord {
                code: "runtime_command_failed".to_owned(),
                message,
                severity: "error".to_owned(),
                recoverable: true,
                related_ref: None,
                suggested_action: None,
            },
        )
    }

    fn status_result(
        &mut self,
        request: &OpsRequest,
        started_at: String,
        resolution: &WorkspaceResolution,
    ) -> OpsResult {
        let mut runtime = (self.runtime_factory)(resolution);
        match runtime.status() {
            Ok(status_payload) => OpsResult {
                schema_version: SCHEMA_VERSION.to_owned(),
                request_id: request.request_id.clone(),
                status: "succeeded".to_owned(),
                action: request.action.clone(),
                workspace_ref: workspace_ref_for_resolution(request, resolution),
                started_at,
                finished_at: now(),
                warnings: Vec::new(),
                errors: Vec::new(),
                result: status_payload,
                route: Some("status_only".to_owned()),
                intake_kind: None,
                scoped_work_item: None,
                completion: Some(Completion {
                    outcome: "unknown".to_owned(),
                    scoped_completion: false,
                    evidence_summary: Vec::new(),
                    missing_evidence: Vec::new(),
                    verification_status: "not_applicable".to_owned(),
                    terminal_outcome_ref: None,
                }),
                evidence_refs: Vec::new(),
                event_cursor: None,
                session_ref: None,
                raw_compat: None,
            },
            Err(error) => self.error_result(
                request,
                started_at,
                resolution,
                ErrorRecord {
                    code: "runtime_command_failed".to_owned(),
                    message: error.to_string(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            ),
        }
    }

    fn enqueue_result(
        &mut self,
        request: &OpsRequest,
        started_at: String,
        resolution: &WorkspaceResolution,
    ) -> OpsResult {
        let Some(agent_factory) = self.agent_factory.as_mut() else {
            return self.error_result(
                request,
                started_at,
                resolution,
                ErrorRecord {
                    code: "unsupported_action".to_owned(),
                    message: "enqueue requires an agent factory for this transport.".to_owned(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            );
        };
        let Some(task) = task_text(&request.input) else {
            return self.error_result(
                request,
                started_at,
                resolution,
                ErrorRecord {
                    code: "invalid_input".to_owned(),
                    message: "enqueue requires input.text or input.payload.task.".to_owned(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            );
        };

        let runtime = (self.runtime_factory)(resolution);
        let workspace = resolution
            .root_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        let mut options = RunOptions::new(workspace.clone(), workspace);
        options.route = route_for_request(request);
        options.intake = intake_for_request(request);
        options.scoped_work_item = request.scoped_work_item.clone();
        if let Some(mode) = resolution.mode.clone() {
            options.millrace_mode = mode;
        }

        let mut agent = agent_factory(resolution, runtime);
        let result = agent.run(&task, options);
        let close_result = agent.close();
        match (result, close_result) {
            (Ok(run_result), Ok(())) => {
                ops_result_from_run_result(request, &run_result, started_at, resolution, &task)
            }
            (Ok(_), Err(error)) | (Err(error), _) => self.error_result(
                request,
                started_at,
                resolution,
                ErrorRecord {
                    code: "runtime_command_failed".to_owned(),
                    message: error.to_string(),
                    severity: "error".to_owned(),
                    recoverable: true,
                    related_ref: None,
                    suggested_action: None,
                },
            ),
        }
    }

    fn success_result(
        &self,
        request: &OpsRequest,
        started_at: String,
        result: JsonObject,
        route: &str,
        session_ref: Option<String>,
    ) -> OpsResult {
        OpsResult {
            schema_version: SCHEMA_VERSION.to_owned(),
            request_id: request.request_id.clone(),
            status: "succeeded".to_owned(),
            action: request.action.clone(),
            workspace_ref: request.workspace_ref.clone(),
            started_at,
            finished_at: now(),
            warnings: Vec::new(),
            errors: Vec::new(),
            result,
            route: Some(route.to_owned()),
            intake_kind: None,
            scoped_work_item: None,
            completion: Some(Completion {
                outcome: "unknown".to_owned(),
                scoped_completion: false,
                evidence_summary: Vec::new(),
                missing_evidence: Vec::new(),
                verification_status: "not_applicable".to_owned(),
                terminal_outcome_ref: None,
            }),
            evidence_refs: Vec::new(),
            event_cursor: None,
            session_ref,
            raw_compat: None,
        }
    }

    fn error_result(
        &self,
        request: &OpsRequest,
        started_at: String,
        resolution: &WorkspaceResolution,
        error: ErrorRecord,
    ) -> OpsResult {
        OpsResult {
            schema_version: SCHEMA_VERSION.to_owned(),
            request_id: request.request_id.clone(),
            status: "failed".to_owned(),
            action: request.action.clone(),
            workspace_ref: workspace_ref_for_resolution(request, resolution),
            started_at,
            finished_at: now(),
            warnings: Vec::new(),
            errors: vec![error],
            result: JsonObject::new(),
            route: None,
            intake_kind: None,
            scoped_work_item: None,
            completion: Some(Completion {
                outcome: "unknown".to_owned(),
                scoped_completion: false,
                evidence_summary: Vec::new(),
                missing_evidence: Vec::new(),
                verification_status: "unverified".to_owned(),
                terminal_outcome_ref: None,
            }),
            evidence_refs: Vec::new(),
            event_cursor: None,
            session_ref: None,
            raw_compat: None,
        }
    }
}

fn ops_result_from_run_result(
    request: &OpsRequest,
    run_result: &RunResult,
    started_at: String,
    resolution: &WorkspaceResolution,
    task: &str,
) -> OpsResult {
    let completion = Completion {
        outcome: run_result.outcome.clone(),
        scoped_completion: run_result.scoped_completion,
        evidence_summary: run_result
            .completion_evidence
            .iter()
            .map(|item| {
                item.iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                    .collect::<JsonObject>()
            })
            .collect(),
        missing_evidence: if run_result.scoped_completion {
            Vec::new()
        } else {
            vec!["positive scoped completion evidence".to_owned()]
        },
        verification_status: if run_result.scoped_completion {
            "verified".to_owned()
        } else {
            "unverified".to_owned()
        },
        terminal_outcome_ref: None,
    };
    OpsResult {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: request.request_id.clone(),
        status: status_for_outcome(run_result).to_owned(),
        action: request.action.clone(),
        workspace_ref: workspace_ref_for_resolution(request, resolution),
        started_at,
        finished_at: now(),
        warnings: run_result
            .warnings
            .iter()
            .map(|warning| warning_from_text(warning))
            .collect(),
        errors: Vec::new(),
        result: JsonObject::from_iter([
            ("task".to_owned(), Value::String(task.to_owned())),
            (
                "output".to_owned(),
                Value::String(run_result.output.clone()),
            ),
            ("event".to_owned(), event_value(run_result)),
            (
                "task_path".to_owned(),
                option_string_value(&run_result.task_path),
            ),
            (
                "status".to_owned(),
                run_result.status.clone().unwrap_or(Value::Null),
            ),
        ]),
        route: Some(run_result.route.clone()),
        intake_kind: Some(run_result.intake_kind.clone()),
        scoped_work_item: run_result
            .scoped_work_item
            .clone()
            .or_else(|| request.scoped_work_item.clone()),
        completion: Some(completion),
        evidence_refs: Vec::new(),
        event_cursor: None,
        session_ref: None,
        raw_compat: raw_compat(run_result),
    }
}

fn workspace_ref_for_resolution(
    request: &OpsRequest,
    resolution: &WorkspaceResolution,
) -> WorkspaceRef {
    WorkspaceRef {
        workspace_id: resolution
            .workspace_id
            .clone()
            .or_else(|| request.workspace_ref.workspace_id.clone()),
        root_path: resolution
            .root_path
            .as_ref()
            .map(|path| path.display().to_string()),
        display_name: resolution
            .display_name
            .clone()
            .or_else(|| request.workspace_ref.display_name.clone()),
        runtime_kind: request.workspace_ref.runtime_kind.clone(),
        mode: resolution
            .mode
            .clone()
            .or_else(|| request.workspace_ref.mode.clone()),
        environment: request.workspace_ref.environment.clone(),
    }
}

fn payload(input: &JsonObject) -> &JsonObject {
    input
        .get("payload")
        .and_then(Value::as_object)
        .unwrap_or(input)
}

fn task_text(input: &JsonObject) -> Option<String> {
    if let Some(text) = optional_string(input.get("text")) {
        return Some(text);
    }
    let nested = input.get("payload")?.as_object()?;
    let selected = if json_truthy(nested.get("task")) {
        nested.get("task")
    } else {
        nested.get("text")
    };
    optional_string(selected)
}

fn route_for_request(request: &OpsRequest) -> String {
    match request.route_preference.as_str() {
        "auto" | "direct" | "millrace" => request.route_preference.clone(),
        _ => "auto".to_owned(),
    }
}

fn intake_for_request(request: &OpsRequest) -> String {
    match request.intake_preference.as_str() {
        "auto" | "probe" | "idea" | "task" => request.intake_preference.clone(),
        _ => "auto".to_owned(),
    }
}

fn status_for_outcome(result: &RunResult) -> &'static str {
    if result.outcome == "completed" && result.scoped_completion {
        return "succeeded";
    }
    match result.outcome.as_str() {
        "blocked" => "blocked",
        "restart_needed" | "crashed" => "failed",
        "incomplete" => "incomplete",
        _ => "failed",
    }
}

fn warning_from_text(text: &str) -> WarningRecord {
    WarningRecord {
        code: "runtime_warning".to_owned(),
        message: text.to_owned(),
        severity: "warning".to_owned(),
        recoverable: true,
        related_ref: None,
        suggested_action: None,
    }
}

fn event_value(result: &RunResult) -> Value {
    let Some(event) = result.event.as_ref() else {
        return Value::Null;
    };
    Value::Object(JsonObject::from_iter([
        ("kind".to_owned(), Value::String(event.kind.clone())),
        (
            "workspace".to_owned(),
            Value::String(event.workspace.clone()),
        ),
        ("reason".to_owned(), Value::String(event.reason.clone())),
    ]))
}

fn raw_compat(result: &RunResult) -> Option<JsonObject> {
    let raw = render_benchmark_result(result).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .as_object()
        .cloned()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
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

fn json_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => number_truthy(value),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Null) | None => false,
    }
}

fn number_truthy(value: &Number) -> bool {
    value
        .as_i64()
        .map(|value| value != 0)
        .or_else(|| value.as_u64().map(|value| value != 0))
        .or_else(|| value.as_f64().map(|value| value != 0.0))
        .unwrap_or(true)
}

fn empty_resolution() -> WorkspaceResolution {
    WorkspaceResolution {
        root_path: None,
        workspace_id: None,
        strategy: "not_required".to_owned(),
        validated: false,
        mode: None,
        display_name: None,
        error_code: None,
    }
}

pub(crate) fn now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    UtcDateTime::from_unix_seconds(seconds).iso8601()
}

#[derive(Debug, Clone, Copy)]
struct UtcDateTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl UtcDateTime {
    fn from_unix_seconds(seconds: u64) -> Self {
        let days = seconds / 86_400;
        let seconds_of_day = seconds % 86_400;
        let (year, month, day) = civil_from_days(days as i64);
        Self {
            year,
            month,
            day,
            hour: (seconds_of_day / 3_600) as u32,
            minute: ((seconds_of_day % 3_600) / 60) as u32,
            second: (seconds_of_day % 60) as u32,
        }
    }

    fn iso8601(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}
