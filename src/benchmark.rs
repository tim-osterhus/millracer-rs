use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::decision::Decision;
use crate::intake::normalize_intake_kind;
pub use crate::monitor::MonitorEvent;
use crate::ops_models::{
    ActorRef, JsonObject, OpsRequest, OpsResult, SCHEMA_VERSION, SourceRef, WorkspaceRef,
    render_ops_result,
};
use crate::scope::ScopedWorkItem;
use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRequest {
    pub task: String,
    pub workspace: Option<PathBuf>,
    pub intake_kind: Option<String>,
    pub scoped_work_item: Option<ScopedWorkItem>,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub route: String,
    pub intake_kind: String,
    pub intake_signals: Vec<String>,
    pub decision: Decision,
    pub output: String,
    pub event: Option<MonitorEvent>,
    pub task_path: Option<String>,
    pub status: Option<Value>,
    pub warnings: Vec<String>,
    pub outcome: String,
    pub scoped_completion: bool,
    pub completion_evidence: Vec<BTreeMap<String, String>>,
    pub scoped_work_item: Option<ScopedWorkItem>,
    pub progress_events: Vec<MonitorEvent>,
    pub task: String,
    pub workspace: String,
    pub cwd: String,
    pub pi_session: String,
    pub millrace_mode: String,
    pub notify_terminal_stages: bool,
}

pub fn parse_benchmark_request(raw: &str) -> MillracerResult<BenchmarkRequest> {
    let payload: Value = serde_json::from_str(raw)?;
    let object = payload
        .as_object()
        .ok_or_else(|| MillracerError::message("external request must be a JSON object"))?;

    let task = optional_string(object.get("task"))
        .or_else(|| optional_string(object.get("prompt")))
        .or_else(|| optional_string(object.get("instructions")))
        .ok_or_else(|| {
            MillracerError::message(
                "external request requires a non-empty task, prompt, or instructions field",
            )
        })?;

    let metadata = metadata_map(object.get("metadata"));
    let scoped_work_item = [
        object.get("scoped_work_item"),
        object.get("work_item"),
        object.get("scope"),
        object.get("scopedWorkItem"),
        object.get("workItem"),
        metadata.get("scoped_work_item"),
    ]
    .into_iter()
    .flatten()
    .find_map(|payload| ScopedWorkItem::from_payload(Some(payload)));
    let intake_kind = optional_string(object.get("intake_kind"))
        .or_else(|| optional_string(metadata.get("intake_kind")))
        .and_then(|value| parse_intake_kind(&value));

    Ok(BenchmarkRequest {
        task,
        workspace: optional_string(object.get("workspace")).map(PathBuf::from),
        intake_kind,
        scoped_work_item,
        metadata: metadata.into_iter().collect(),
    })
}

pub fn render_benchmark_result(result: &RunResult) -> MillracerResult<String> {
    Ok(serde_json::to_string_pretty(&run_result_json(result))?)
}

pub fn parse_legacy_request_as_ops(
    raw: &str,
    request_id: Option<&str>,
) -> MillracerResult<OpsRequest> {
    let payload: Value = serde_json::from_str(raw)?;
    let request = parse_benchmark_request(raw)?;
    let mut metadata: JsonObject = request.metadata.clone().into_iter().collect();
    metadata.insert(
        "legacy_request".to_owned(),
        payload
            .as_object()
            .cloned()
            .map(Value::Object)
            .unwrap_or_else(|| Value::Object(JsonObject::new())),
    );

    Ok(OpsRequest {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: request_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(default_legacy_request_id),
        action: "enqueue".to_owned(),
        workspace_ref: WorkspaceRef {
            workspace_id: None,
            root_path: request
                .workspace
                .as_ref()
                .map(|path| path.display().to_string()),
            display_name: None,
            runtime_kind: "local".to_owned(),
            mode: None,
            environment: None,
        },
        source: SourceRef {
            kind: "benchmark_compat".to_owned(),
            surface: None,
            adapter_id: None,
            conversation_id: None,
            message_id: None,
            parent_request_id: None,
            trace_ref: None,
        },
        input: JsonObject::from_iter([
            (
                "kind".to_owned(),
                Value::String("legacy_benchmark".to_owned()),
            ),
            ("text".to_owned(), Value::String(request.task)),
        ]),
        client_name: None,
        client_version: None,
        actor: None::<ActorRef>,
        route_preference: "millrace".to_owned(),
        intake_preference: request.intake_kind.unwrap_or_else(|| "auto".to_owned()),
        scoped_work_item: request.scoped_work_item,
        expected_evidence: Vec::new(),
        context_refs: Vec::new(),
        options: JsonObject::new(),
        metadata,
        idempotency_key: None,
    })
}

pub fn ops_result_to_legacy_json(result: &OpsResult) -> MillracerResult<String> {
    Ok(serde_json::to_string_pretty(&render_ops_result(result))?)
}

fn run_result_json(result: &RunResult) -> Value {
    let mut payload = Map::new();
    payload.insert("cwd".to_owned(), Value::String(result.cwd.clone()));
    payload.insert("decision".to_owned(), decision_json(result));
    payload.insert("event".to_owned(), event_json(result.event.as_ref()));
    payload.insert(
        "completion_evidence".to_owned(),
        completion_evidence_json(&result.completion_evidence),
    );
    payload.insert(
        "intake_kind".to_owned(),
        Value::String(result.intake_kind.clone()),
    );
    payload.insert(
        "intake_signals".to_owned(),
        string_array(&result.intake_signals),
    );
    payload.insert(
        "millrace_mode".to_owned(),
        Value::String(result.millrace_mode.clone()),
    );
    payload.insert(
        "notify_terminal_stages".to_owned(),
        Value::Bool(result.notify_terminal_stages),
    );
    payload.insert("output".to_owned(), Value::String(result.output.clone()));
    payload.insert("outcome".to_owned(), Value::String(result.outcome.clone()));
    payload.insert(
        "pi_session".to_owned(),
        Value::String(result.pi_session.clone()),
    );
    payload.insert(
        "progress_events".to_owned(),
        Value::Array(
            result
                .progress_events
                .iter()
                .map(|event| event_json(Some(event)))
                .collect(),
        ),
    );
    payload.insert("route".to_owned(), Value::String(result.route.clone()));
    payload.insert(
        "scoped_completion".to_owned(),
        Value::Bool(result.scoped_completion),
    );
    payload.insert(
        "scoped_work_item".to_owned(),
        result
            .scoped_work_item
            .as_ref()
            .map(ScopedWorkItem::to_json_value)
            .unwrap_or(Value::Null),
    );
    payload.insert(
        "status".to_owned(),
        result.status.clone().unwrap_or(Value::Null),
    );
    payload.insert("task".to_owned(), Value::String(result.task.clone()));
    payload.insert(
        "task_path".to_owned(),
        result
            .task_path
            .as_ref()
            .map(|path| Value::String(path.clone()))
            .unwrap_or(Value::Null),
    );
    payload.insert("warnings".to_owned(), string_array(&result.warnings));
    payload.insert(
        "workspace".to_owned(),
        Value::String(result.workspace.clone()),
    );
    Value::Object(payload)
}

fn completion_evidence_json(evidence: &[BTreeMap<String, String>]) -> Value {
    Value::Array(
        evidence
            .iter()
            .map(|item| {
                Value::Object(
                    item.iter()
                        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn decision_json(result: &RunResult) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "custom_loop_needed".to_owned(),
        Value::Bool(result.decision.custom_loop_needed),
    );
    payload.insert(
        "intake_kind".to_owned(),
        result
            .decision
            .intake_kind
            .as_ref()
            .map(|kind| Value::String(kind.clone()))
            .unwrap_or(Value::Null),
    );
    payload.insert(
        "mode".to_owned(),
        Value::String(result.decision.mode.clone()),
    );
    payload.insert(
        "notes".to_owned(),
        Value::String(result.decision.notes.clone()),
    );
    payload.insert(
        "route".to_owned(),
        Value::String(result.decision.route.clone()),
    );
    payload.insert(
        "signals".to_owned(),
        if result.decision.signals.is_empty() {
            string_array(&result.intake_signals)
        } else {
            string_array(&result.decision.signals)
        },
    );
    payload.insert("why".to_owned(), Value::String(result.decision.why.clone()));
    Value::Object(payload)
}

fn event_json(event: Option<&MonitorEvent>) -> Value {
    let Some(event) = event else {
        return Value::Null;
    };
    let mut payload = Map::new();
    payload.insert("kind".to_owned(), Value::String(event.kind.clone()));
    payload.insert("reason".to_owned(), Value::String(event.reason.clone()));
    payload.insert(
        "workspace".to_owned(),
        Value::String(event.workspace.clone()),
    );
    Value::Object(payload)
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::String(value.clone()))
            .collect(),
    )
}

fn metadata_map(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_intake_kind(value: &str) -> Option<String> {
    normalize_intake_kind(value, false).map(|kind| kind.as_str().to_owned())
}

fn default_legacy_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("req-legacy-{nanos}")
}
