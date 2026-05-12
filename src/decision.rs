use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub route: String,
    pub why: String,
    pub mode: String,
    pub custom_loop_needed: bool,
    pub notes: String,
    pub intake_kind: Option<String>,
    pub signals: Vec<String>,
}

impl Decision {
    pub fn new(route: impl Into<String>, why: impl Into<String>) -> Self {
        Self {
            route: route.into(),
            why: why.into(),
            mode: "default_pi".to_owned(),
            custom_loop_needed: false,
            notes: String::new(),
            intake_kind: None,
            signals: Vec::new(),
        }
    }
}

pub fn parse_decision(raw: &str) -> Decision {
    if let Some(payload) = load_decision_payload(raw) {
        let route_value = truthy_value(payload.get("decision")).or_else(|| payload.get("route"));
        let route = normalize_route(route_value);
        let why = string_value(payload.get("why"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Pi selected this route.".to_owned());
        let mode = string_value(payload.get("mode"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "default_pi".to_owned());

        return Decision {
            route,
            why,
            mode,
            custom_loop_needed: json_truthy(payload.get("custom_loop_needed")),
            notes: string_value(payload.get("notes")).unwrap_or_default(),
            intake_kind: normalize_intake_kind(payload.get("intake_kind")),
            signals: normalize_signals(payload.get("signals")),
        };
    }

    let route = route_line(raw).unwrap_or_else(|| "direct".to_owned());
    let why = why_line(raw).unwrap_or_else(|| "Pi returned unstructured output.".to_owned());
    Decision::new(route, why)
}

fn load_decision_payload(raw: &str) -> Option<Map<String, Value>> {
    let mut candidates = Vec::new();
    if let Some(fenced) = fenced_json_body(raw) {
        candidates.push(fenced);
    }
    candidates.push(raw.trim().to_owned());

    for candidate in candidates {
        if !candidate.starts_with('{') {
            continue;
        }
        let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(&candidate) else {
            continue;
        };
        return Some(payload);
    }
    None
}

fn fenced_json_body(raw: &str) -> Option<String> {
    let mut remainder = raw;
    while let Some(start) = remainder.find("```") {
        let after_fence = &remainder[start + 3..];
        let mut body_start = after_fence.trim_start();
        if body_start.len() >= 4 && body_start[..4].eq_ignore_ascii_case("json") {
            body_start = body_start[4..].trim_start();
        }
        let end = body_start.find("```")?;
        let body = body_start[..end].trim();
        if body.starts_with('{') {
            return Some(body.to_owned());
        }
        remainder = &body_start[end + 3..];
    }
    None
}

fn route_line(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("decision") {
            continue;
        }
        let route = value.trim().to_ascii_lowercase();
        if route == "direct" || route == "millrace" {
            return Some(route);
        }
    }
    None
}

fn why_line(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("why") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn normalize_route(value: Option<&Value>) -> String {
    let route = string_value(value).unwrap_or_default().to_ascii_lowercase();
    match route.as_str() {
        "direct" | "millrace" => route,
        _ => "direct".to_owned(),
    }
}

fn normalize_intake_kind(value: Option<&Value>) -> Option<String> {
    let intake_kind = string_value(value)?.to_ascii_lowercase();
    match intake_kind.as_str() {
        "probe" | "idea" | "task" => Some(intake_kind),
        _ => None,
    }
}

fn normalize_signals(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| string_value(Some(item)))
        .filter(|signal| !signal.is_empty())
        .collect()
}

fn truthy_value(value: Option<&Value>) -> Option<&Value> {
    value.filter(|item| json_truthy(Some(item)))
}

fn json_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Null) | None => false,
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    let value = match value? {
        Value::String(value) => value.clone(),
        Value::Null => return None,
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => return None,
    };
    Some(value.trim().to_owned())
}
