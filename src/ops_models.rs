use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::scope::ScopedWorkItem;
use crate::{MillracerError, MillracerResult};

pub const SCHEMA_VERSION: &str = "millracer.ops.v0.2";

pub type JsonObject = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRef {
    pub workspace_id: Option<String>,
    pub root_path: Option<String>,
    pub display_name: Option<String>,
    pub runtime_kind: String,
    pub mode: Option<String>,
    pub environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: String,
    pub surface: Option<String>,
    pub adapter_id: Option<String>,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub trace_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRef {
    pub kind: String,
    pub display_name: Option<String>,
    pub actor_id: Option<String>,
    pub metadata: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningRecord {
    pub code: String,
    pub message: String,
    pub severity: String,
    pub recoverable: bool,
    pub related_ref: Option<String>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub code: String,
    pub message: String,
    pub severity: String,
    pub recoverable: bool,
    pub related_ref: Option<String>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub outcome: String,
    pub scoped_completion: bool,
    pub evidence_summary: Vec<JsonObject>,
    pub missing_evidence: Vec<String>,
    pub verification_status: String,
    pub terminal_outcome_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpsRequest {
    pub schema_version: String,
    pub request_id: String,
    pub action: String,
    pub workspace_ref: WorkspaceRef,
    pub source: SourceRef,
    pub input: JsonObject,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub actor: Option<ActorRef>,
    pub route_preference: String,
    pub intake_preference: String,
    pub scoped_work_item: Option<ScopedWorkItem>,
    pub expected_evidence: Vec<String>,
    pub context_refs: Vec<String>,
    pub options: JsonObject,
    pub metadata: JsonObject,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpsResult {
    pub schema_version: String,
    pub request_id: String,
    pub status: String,
    pub action: String,
    pub workspace_ref: WorkspaceRef,
    pub started_at: String,
    pub finished_at: String,
    pub warnings: Vec<WarningRecord>,
    pub errors: Vec<ErrorRecord>,
    pub result: JsonObject,
    pub route: Option<String>,
    pub intake_kind: Option<String>,
    pub scoped_work_item: Option<ScopedWorkItem>,
    pub completion: Option<Completion>,
    pub evidence_refs: Vec<String>,
    pub event_cursor: Option<String>,
    pub session_ref: Option<String>,
    pub raw_compat: Option<JsonObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpsEventFrame {
    pub schema_version: String,
    pub request_id: String,
    pub event_id: String,
    pub sequence: i64,
    pub timestamp: String,
    pub event_type: String,
    pub severity: String,
    pub message: String,
    pub payload: JsonObject,
    pub cursor: Option<String>,
}

pub fn parse_ops_request(raw: &str) -> MillracerResult<OpsRequest> {
    let payload = loads_object(raw, "ops request")?;
    parse_ops_request_payload(&payload)
}

pub fn parse_ops_request_payload(payload: &JsonObject) -> MillracerResult<OpsRequest> {
    require_schema(payload)?;

    let metadata = mapping_or_empty(payload.get("metadata"));
    Ok(OpsRequest {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: required_string(payload, "request_id")?,
        action: required_string(payload, "action")?,
        workspace_ref: parse_workspace_ref(required_mapping(payload, "workspace_ref")?),
        source: parse_source_ref(required_mapping(payload, "source")?)?,
        input: mapping_or_empty(payload.get("input")),
        client_name: optional_string(payload.get("client_name")),
        client_version: optional_string(payload.get("client_version")),
        actor: parse_actor_ref(payload.get("actor"))?,
        route_preference: optional_string(payload.get("route_preference"))
            .unwrap_or_else(|| "auto".to_owned()),
        intake_preference: optional_string(payload.get("intake_preference"))
            .unwrap_or_else(|| "auto".to_owned()),
        scoped_work_item: scoped_work_item_from_aliases(payload, &metadata),
        expected_evidence: string_list(payload.get("expected_evidence")),
        context_refs: string_list(payload.get("context_refs")),
        options: mapping_or_empty(payload.get("options")),
        metadata,
        idempotency_key: optional_string(payload.get("idempotency_key")),
    })
}

pub fn parse_ops_result(raw: &str) -> MillracerResult<OpsResult> {
    let payload = loads_object(raw, "ops result")?;
    parse_ops_result_payload(&payload)
}

pub fn parse_ops_result_payload(payload: &JsonObject) -> MillracerResult<OpsResult> {
    require_schema(payload)?;

    Ok(OpsResult {
        schema_version: SCHEMA_VERSION.to_owned(),
        request_id: required_string(payload, "request_id")?,
        status: required_string(payload, "status")?,
        action: required_string(payload, "action")?,
        workspace_ref: parse_workspace_ref(required_mapping(payload, "workspace_ref")?),
        started_at: required_string(payload, "started_at")?,
        finished_at: required_string(payload, "finished_at")?,
        warnings: list_or_empty(payload.get("warnings"))
            .iter()
            .map(parse_warning)
            .collect::<MillracerResult<Vec<_>>>()?,
        errors: list_or_empty(payload.get("errors"))
            .iter()
            .map(parse_error)
            .collect::<MillracerResult<Vec<_>>>()?,
        result: mapping_or_empty(payload.get("result")),
        route: optional_string(payload.get("route")),
        intake_kind: optional_string(payload.get("intake_kind")),
        scoped_work_item: ScopedWorkItem::from_payload(payload.get("scoped_work_item")),
        completion: parse_completion(payload.get("completion"))?,
        evidence_refs: string_list(payload.get("evidence_refs")),
        event_cursor: optional_string(payload.get("event_cursor")),
        session_ref: optional_string(payload.get("session_ref")),
        raw_compat: optional_mapping(payload.get("raw_compat")),
    })
}

pub fn render_ops_request(request: &OpsRequest) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert(
        "schema_version".to_owned(),
        Value::String(request.schema_version.clone()),
    );
    rendered.insert(
        "request_id".to_owned(),
        Value::String(request.request_id.clone()),
    );
    rendered.insert(
        "workspace_ref".to_owned(),
        render_workspace_ref(&request.workspace_ref),
    );
    rendered.insert("source".to_owned(), render_source_ref(&request.source));
    rendered.insert("action".to_owned(), Value::String(request.action.clone()));
    rendered.insert(
        "route_preference".to_owned(),
        Value::String(request.route_preference.clone()),
    );
    rendered.insert(
        "intake_preference".to_owned(),
        Value::String(request.intake_preference.clone()),
    );
    rendered.insert("input".to_owned(), Value::Object(request.input.clone()));
    set_string_if_some(&mut rendered, "client_name", &request.client_name);
    set_string_if_some(&mut rendered, "client_version", &request.client_version);
    if let Some(actor) = &request.actor {
        rendered.insert("actor".to_owned(), render_actor_ref(actor));
    }
    if let Some(scoped_work_item) = &request.scoped_work_item {
        rendered.insert(
            "scoped_work_item".to_owned(),
            scoped_work_item.to_json_value(),
        );
    }
    if !request.expected_evidence.is_empty() {
        rendered.insert(
            "expected_evidence".to_owned(),
            string_array(&request.expected_evidence),
        );
    }
    if !request.context_refs.is_empty() {
        rendered.insert(
            "context_refs".to_owned(),
            string_array(&request.context_refs),
        );
    }
    if !request.options.is_empty() {
        rendered.insert("options".to_owned(), Value::Object(request.options.clone()));
    }
    if !request.metadata.is_empty() {
        rendered.insert(
            "metadata".to_owned(),
            Value::Object(request.metadata.clone()),
        );
    }
    set_string_if_some(&mut rendered, "idempotency_key", &request.idempotency_key);
    Value::Object(rendered)
}

pub fn render_ops_result(result: &OpsResult) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert(
        "schema_version".to_owned(),
        Value::String(result.schema_version.clone()),
    );
    rendered.insert(
        "request_id".to_owned(),
        Value::String(result.request_id.clone()),
    );
    rendered.insert("status".to_owned(), Value::String(result.status.clone()));
    rendered.insert("action".to_owned(), Value::String(result.action.clone()));
    rendered.insert(
        "workspace_ref".to_owned(),
        render_workspace_ref(&result.workspace_ref),
    );
    rendered.insert(
        "started_at".to_owned(),
        Value::String(result.started_at.clone()),
    );
    rendered.insert(
        "finished_at".to_owned(),
        Value::String(result.finished_at.clone()),
    );
    rendered.insert(
        "warnings".to_owned(),
        Value::Array(result.warnings.iter().map(render_warning).collect()),
    );
    rendered.insert(
        "errors".to_owned(),
        Value::Array(result.errors.iter().map(render_error).collect()),
    );
    rendered.insert("result".to_owned(), Value::Object(result.result.clone()));
    set_string_if_some(&mut rendered, "route", &result.route);
    set_string_if_some(&mut rendered, "intake_kind", &result.intake_kind);
    if let Some(scoped_work_item) = &result.scoped_work_item {
        rendered.insert(
            "scoped_work_item".to_owned(),
            scoped_work_item.to_json_value(),
        );
    }
    if let Some(completion) = &result.completion {
        rendered.insert("completion".to_owned(), render_completion(completion));
    }
    if !result.evidence_refs.is_empty() {
        rendered.insert(
            "evidence_refs".to_owned(),
            string_array(&result.evidence_refs),
        );
    }
    set_string_if_some(&mut rendered, "event_cursor", &result.event_cursor);
    set_string_if_some(&mut rendered, "session_ref", &result.session_ref);
    if let Some(raw_compat) = &result.raw_compat {
        rendered.insert("raw_compat".to_owned(), Value::Object(raw_compat.clone()));
    }
    Value::Object(rendered)
}

pub fn render_ops_event_frame(frame: &OpsEventFrame) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert(
        "schema_version".to_owned(),
        Value::String(frame.schema_version.clone()),
    );
    rendered.insert(
        "request_id".to_owned(),
        Value::String(frame.request_id.clone()),
    );
    rendered.insert("event_id".to_owned(), Value::String(frame.event_id.clone()));
    rendered.insert(
        "sequence".to_owned(),
        Value::Number(Number::from(frame.sequence)),
    );
    rendered.insert(
        "timestamp".to_owned(),
        Value::String(frame.timestamp.clone()),
    );
    rendered.insert(
        "event_type".to_owned(),
        Value::String(frame.event_type.clone()),
    );
    rendered.insert("severity".to_owned(), Value::String(frame.severity.clone()));
    rendered.insert("message".to_owned(), Value::String(frame.message.clone()));
    rendered.insert("payload".to_owned(), Value::Object(frame.payload.clone()));
    set_string_if_some(&mut rendered, "cursor", &frame.cursor);
    Value::Object(rendered)
}

fn loads_object(raw: &str, label: &str) -> MillracerResult<JsonObject> {
    match serde_json::from_str::<Value>(raw)? {
        Value::Object(payload) => Ok(payload),
        _ => Err(MillracerError::message(format!(
            "{label} must be a JSON object"
        ))),
    }
}

fn require_schema(payload: &JsonObject) -> MillracerResult<()> {
    match payload.get("schema_version") {
        Some(Value::String(schema_version)) if schema_version == SCHEMA_VERSION => Ok(()),
        value => Err(MillracerError::message(format!(
            "unsupported ops schema: {}",
            schema_message(value)
        ))),
    }
}

fn schema_message(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
        None => "null".to_owned(),
    }
}

fn required_string(payload: &JsonObject, key: &str) -> MillracerResult<String> {
    optional_string(payload.get(key))
        .ok_or_else(|| MillracerError::message(format!("ops payload requires non-empty {key}")))
}

fn required_mapping<'a>(payload: &'a JsonObject, key: &str) -> MillracerResult<&'a JsonObject> {
    match payload.get(key) {
        Some(Value::Object(value)) => Ok(value),
        _ => Err(MillracerError::message(format!(
            "ops payload requires object {key}"
        ))),
    }
}

fn mapping_or_empty(value: Option<&Value>) -> JsonObject {
    match value {
        Some(Value::Object(value)) => value.clone(),
        _ => JsonObject::new(),
    }
}

fn optional_mapping(value: Option<&Value>) -> Option<JsonObject> {
    match value {
        Some(Value::Object(value)) => Some(value.clone()),
        _ => None,
    }
}

fn list_or_empty(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(value)) => value,
        _ => &[],
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    list_or_empty(value)
        .iter()
        .filter_map(|item| optional_string(Some(item)))
        .collect()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn scoped_work_item_from_aliases(
    payload: &JsonObject,
    metadata: &JsonObject,
) -> Option<ScopedWorkItem> {
    [
        payload.get("scoped_work_item"),
        payload.get("work_item"),
        payload.get("scope"),
        payload.get("scopedWorkItem"),
        payload.get("workItem"),
        metadata.get("scoped_work_item"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| ScopedWorkItem::from_payload(Some(value)))
}

fn parse_workspace_ref(payload: &JsonObject) -> WorkspaceRef {
    WorkspaceRef {
        workspace_id: optional_string(payload.get("workspace_id")),
        root_path: optional_string(payload.get("root_path")),
        display_name: optional_string(payload.get("display_name")),
        runtime_kind: optional_string(payload.get("runtime_kind"))
            .unwrap_or_else(|| "local".to_owned()),
        mode: optional_string(payload.get("mode")),
        environment: optional_string(payload.get("environment")),
    }
}

fn parse_source_ref(payload: &JsonObject) -> MillracerResult<SourceRef> {
    Ok(SourceRef {
        kind: required_string(payload, "kind")?,
        surface: optional_string(payload.get("surface")),
        adapter_id: optional_string(payload.get("adapter_id")),
        conversation_id: optional_string(payload.get("conversation_id")),
        message_id: optional_string(payload.get("message_id")),
        parent_request_id: optional_string(payload.get("parent_request_id")),
        trace_ref: optional_string(payload.get("trace_ref")),
    })
}

fn parse_actor_ref(value: Option<&Value>) -> MillracerResult<Option<ActorRef>> {
    let Some(Value::Object(payload)) = value else {
        return Ok(None);
    };
    Ok(Some(ActorRef {
        kind: required_string(payload, "kind")?,
        display_name: optional_string(payload.get("display_name")),
        actor_id: optional_string(payload.get("actor_id")),
        metadata: mapping_or_empty(payload.get("metadata")),
    }))
}

fn parse_warning(value: &Value) -> MillracerResult<WarningRecord> {
    let Value::Object(payload) = value else {
        return Err(MillracerError::message("warning records must be objects"));
    };
    Ok(WarningRecord {
        code: required_string(payload, "code")?,
        message: required_string(payload, "message")?,
        severity: optional_string(payload.get("severity")).unwrap_or_else(|| "warning".to_owned()),
        recoverable: python_truthy(payload.get("recoverable"), true),
        related_ref: optional_string(payload.get("related_ref")),
        suggested_action: optional_string(payload.get("suggested_action")),
    })
}

fn parse_error(value: &Value) -> MillracerResult<ErrorRecord> {
    let Value::Object(payload) = value else {
        return Err(MillracerError::message("error records must be objects"));
    };
    Ok(ErrorRecord {
        code: required_string(payload, "code")?,
        message: required_string(payload, "message")?,
        severity: optional_string(payload.get("severity")).unwrap_or_else(|| "error".to_owned()),
        recoverable: python_truthy(payload.get("recoverable"), false),
        related_ref: optional_string(payload.get("related_ref")),
        suggested_action: optional_string(payload.get("suggested_action")),
    })
}

fn parse_completion(value: Option<&Value>) -> MillracerResult<Option<Completion>> {
    let Some(Value::Object(payload)) = value else {
        return Ok(None);
    };
    Ok(Some(Completion {
        outcome: required_string(payload, "outcome")?,
        scoped_completion: python_truthy(payload.get("scoped_completion"), false),
        evidence_summary: list_or_empty(payload.get("evidence_summary"))
            .iter()
            .filter_map(|item| item.as_object().cloned())
            .collect(),
        missing_evidence: string_list(payload.get("missing_evidence")),
        verification_status: optional_string(payload.get("verification_status"))
            .unwrap_or_else(|| "unverified".to_owned()),
        terminal_outcome_ref: optional_string(payload.get("terminal_outcome_ref")),
    }))
}

fn python_truthy(value: Option<&Value>, default: bool) -> bool {
    match value {
        None => default,
        Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => number_truthy(value),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
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

fn render_workspace_ref(workspace_ref: &WorkspaceRef) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert(
        "runtime_kind".to_owned(),
        Value::String(workspace_ref.runtime_kind.clone()),
    );
    set_string_if_some(&mut rendered, "workspace_id", &workspace_ref.workspace_id);
    set_string_if_some(&mut rendered, "root_path", &workspace_ref.root_path);
    set_string_if_some(&mut rendered, "display_name", &workspace_ref.display_name);
    set_string_if_some(&mut rendered, "mode", &workspace_ref.mode);
    set_string_if_some(&mut rendered, "environment", &workspace_ref.environment);
    Value::Object(rendered)
}

fn render_source_ref(source: &SourceRef) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert("kind".to_owned(), Value::String(source.kind.clone()));
    set_string_if_some(&mut rendered, "surface", &source.surface);
    set_string_if_some(&mut rendered, "adapter_id", &source.adapter_id);
    set_string_if_some(&mut rendered, "conversation_id", &source.conversation_id);
    set_string_if_some(&mut rendered, "message_id", &source.message_id);
    set_string_if_some(
        &mut rendered,
        "parent_request_id",
        &source.parent_request_id,
    );
    set_string_if_some(&mut rendered, "trace_ref", &source.trace_ref);
    Value::Object(rendered)
}

fn render_actor_ref(actor: &ActorRef) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert("kind".to_owned(), Value::String(actor.kind.clone()));
    set_string_if_some(&mut rendered, "display_name", &actor.display_name);
    set_string_if_some(&mut rendered, "actor_id", &actor.actor_id);
    if !actor.metadata.is_empty() {
        rendered.insert("metadata".to_owned(), Value::Object(actor.metadata.clone()));
    }
    Value::Object(rendered)
}

fn render_warning(warning: &WarningRecord) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert("code".to_owned(), Value::String(warning.code.clone()));
    rendered.insert("message".to_owned(), Value::String(warning.message.clone()));
    rendered.insert(
        "severity".to_owned(),
        Value::String(warning.severity.clone()),
    );
    rendered.insert("recoverable".to_owned(), Value::Bool(warning.recoverable));
    set_string_if_some(&mut rendered, "related_ref", &warning.related_ref);
    set_string_if_some(&mut rendered, "suggested_action", &warning.suggested_action);
    Value::Object(rendered)
}

fn render_error(error: &ErrorRecord) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert("code".to_owned(), Value::String(error.code.clone()));
    rendered.insert("message".to_owned(), Value::String(error.message.clone()));
    rendered.insert("severity".to_owned(), Value::String(error.severity.clone()));
    rendered.insert("recoverable".to_owned(), Value::Bool(error.recoverable));
    set_string_if_some(&mut rendered, "related_ref", &error.related_ref);
    set_string_if_some(&mut rendered, "suggested_action", &error.suggested_action);
    Value::Object(rendered)
}

fn render_completion(completion: &Completion) -> Value {
    let mut rendered = JsonObject::new();
    rendered.insert(
        "outcome".to_owned(),
        Value::String(completion.outcome.clone()),
    );
    rendered.insert(
        "scoped_completion".to_owned(),
        Value::Bool(completion.scoped_completion),
    );
    rendered.insert(
        "evidence_summary".to_owned(),
        Value::Array(
            completion
                .evidence_summary
                .iter()
                .cloned()
                .map(Value::Object)
                .collect(),
        ),
    );
    rendered.insert(
        "missing_evidence".to_owned(),
        string_array(&completion.missing_evidence),
    );
    rendered.insert(
        "verification_status".to_owned(),
        Value::String(completion.verification_status.clone()),
    );
    rendered.insert(
        "terminal_outcome_ref".to_owned(),
        option_string_value(&completion.terminal_outcome_ref),
    );
    Value::Object(rendered)
}

fn set_string_if_some(target: &mut JsonObject, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), Value::String(value.clone()));
    }
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
