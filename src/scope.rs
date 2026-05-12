use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedWorkItem {
    pub item_id: String,
    pub title: Option<String>,
    pub source_queue: Option<String>,
    pub spec_path: Option<String>,
    pub completion_ref: Option<String>,
    pub constraints: Vec<String>,
}

impl ScopedWorkItem {
    pub fn from_payload(payload: Option<&Value>) -> Option<Self> {
        let payload = payload?.as_object()?;
        let item_id = optional_string(payload.get("item_id"))
            .or_else(|| optional_string(payload.get("id")))?;
        let constraints = payload
            .get("constraints")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| optional_string(Some(item)))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        Some(Self {
            item_id,
            title: optional_string(payload.get("title")),
            source_queue: optional_string(payload.get("source_queue")),
            spec_path: optional_string(payload.get("spec_path")),
            completion_ref: optional_string(payload.get("completion_ref")),
            constraints,
        })
    }

    pub fn to_json_value(&self) -> Value {
        let mut payload = Map::new();
        payload.insert(
            "completion_ref".to_owned(),
            option_value(&self.completion_ref),
        );
        payload.insert(
            "constraints".to_owned(),
            Value::Array(
                self.constraints
                    .iter()
                    .map(|item| Value::String(item.clone()))
                    .collect(),
            ),
        );
        payload.insert("item_id".to_owned(), Value::String(self.item_id.clone()));
        payload.insert("source_queue".to_owned(), option_value(&self.source_queue));
        payload.insert("spec_path".to_owned(), option_value(&self.spec_path));
        payload.insert("title".to_owned(), option_value(&self.title));
        Value::Object(payload)
    }
}

pub fn render_scoped_work(scoped_work_item: Option<&ScopedWorkItem>) -> String {
    let Some(scoped_work_item) = scoped_work_item else {
        return "Scoped-Work:\n- Item-ID: none\n".to_owned();
    };

    let mut lines = vec![
        "Scoped-Work:".to_owned(),
        format!("- Item-ID: {}", scoped_work_item.item_id),
    ];
    if let Some(title) = &scoped_work_item.title {
        lines.push(format!("- Title: {title}"));
    }
    if let Some(source_queue) = &scoped_work_item.source_queue {
        lines.push(format!("- Source-Queue: {source_queue}"));
    }
    if let Some(spec_path) = &scoped_work_item.spec_path {
        lines.push(format!("- Spec-Path: {spec_path}"));
    }
    if let Some(completion_ref) = &scoped_work_item.completion_ref {
        lines.push(format!("- Completion-Ref: {completion_ref}"));
    }
    for constraint in &scoped_work_item.constraints {
        lines.push(format!("- Constraint: {constraint}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn scoped_work_json(scoped_work_item: Option<&ScopedWorkItem>) -> Option<String> {
    let scoped_work_item = scoped_work_item?;
    serde_json::to_string_pretty(&scoped_work_item.to_json_value()).ok()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn option_value(value: &Option<String>) -> Value {
    value
        .as_ref()
        .map(|value| Value::String(value.clone()))
        .unwrap_or(Value::Null)
}
