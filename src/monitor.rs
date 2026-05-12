use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorEvent {
    pub kind: String,
    pub workspace: String,
    pub reason: String,
}

impl MonitorEvent {
    pub fn new(
        kind: impl Into<String>,
        workspace: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            workspace: workspace.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgressKey {
    kind: String,
    workspace: String,
    reason: String,
    active_work_item_id: String,
    queued_work: String,
    closure_target_root_spec_id: String,
}

pub struct DaemonMonitor<L, S = fn(f64)>
where
    L: FnMut() -> MillracerResult<Map<String, Value>>,
    S: FnMut(f64),
{
    status_loader: L,
    poll_interval_seconds: f64,
    sleep: S,
    notify_terminal_stages: bool,
    last_progress_key: Option<ProgressKey>,
}

impl<L> DaemonMonitor<L, fn(f64)>
where
    L: FnMut() -> MillracerResult<Map<String, Value>>,
{
    pub fn new(status_loader: L) -> Self {
        Self::with_sleep(status_loader, 2.0, sleep_seconds as fn(f64))
    }

    pub fn with_poll_interval(status_loader: L, poll_interval_seconds: f64) -> Self {
        Self::with_sleep(
            status_loader,
            poll_interval_seconds,
            sleep_seconds as fn(f64),
        )
    }
}

impl<L, S> DaemonMonitor<L, S>
where
    L: FnMut() -> MillracerResult<Map<String, Value>>,
    S: FnMut(f64),
{
    pub fn with_sleep(status_loader: L, poll_interval_seconds: f64, sleep: S) -> Self {
        Self {
            status_loader,
            poll_interval_seconds,
            sleep,
            notify_terminal_stages: true,
            last_progress_key: None,
        }
    }

    pub fn with_notify_terminal_stages(mut self, notify_terminal_stages: bool) -> Self {
        self.notify_terminal_stages = notify_terminal_stages;
        self
    }

    pub fn set_notify_terminal_stages(&mut self, notify_terminal_stages: bool) {
        self.notify_terminal_stages = notify_terminal_stages;
    }

    pub fn wait(&mut self, timeout_seconds: f64) -> MillracerResult<MonitorEvent> {
        let now = Instant::now();
        let deadline = now
            .checked_add(duration_from_seconds(timeout_seconds))
            .unwrap_or(now);

        loop {
            let payload = (self.status_loader)()?;
            let event = classify_status(&payload, self.notify_terminal_stages);
            if let Some(event) = event {
                if event.kind == "stage_progress" && self.already_seen_progress(&payload, &event) {
                } else {
                    return Ok(event);
                }
            } else {
                self.last_progress_key = None;
            }

            if Instant::now() >= deadline {
                let workspace = truthy_string_or(payload.get("workspace"), "");
                return Err(MillracerError::message(format!(
                    "timed out waiting for Millrace daemon event in {workspace}"
                )));
            }

            (self.sleep)(self.poll_interval_seconds);
        }
    }

    fn already_seen_progress(
        &mut self,
        payload: &Map<String, Value>,
        event: &MonitorEvent,
    ) -> bool {
        let key = ProgressKey {
            kind: event.kind.clone(),
            workspace: event.workspace.clone(),
            reason: event.reason.clone(),
            active_work_item_id: truthy_string_or(payload.get("active_work_item_id"), ""),
            queued_work: queued_work(payload).to_string(),
            closure_target_root_spec_id: truthy_string_or(
                payload.get("closure_target_root_spec_id"),
                "",
            ),
        };
        if self.last_progress_key.as_ref() == Some(&key) {
            return true;
        }
        self.last_progress_key = Some(key);
        false
    }
}

pub fn classify_status(
    payload: &Map<String, Value>,
    notify_terminal_stages: bool,
) -> Option<MonitorEvent> {
    let workspace = truthy_string_or(payload.get("workspace"), "");
    let failure_class = payload
        .get("current_failure_class")
        .filter(|value| truthy(value));
    if is_bool(payload.get("blocked_idle"), true) || failure_class.is_some() {
        let reason = failure_class
            .map(python_string)
            .unwrap_or_else(|| "blocked idle".to_owned());
        return Some(MonitorEvent::new("blocked", workspace, reason));
    }

    if is_bool(payload.get("process_running"), false) && queued_work(payload) > 0 {
        let mut reason = "daemon stopped with queued work".to_owned();
        if payload
            .get("runtime_ownership_lock")
            .and_then(Value::as_str)
            == Some("stale")
        {
            reason.push_str(" and stale runtime ownership lock");
        }
        return Some(MonitorEvent::new("restart_needed", workspace, reason));
    }

    let planning_marker = truthy_string_or(payload.get("planning_status_marker"), "");
    if planning_marker.trim() == "### ARBITER_COMPLETE" {
        return Some(MonitorEvent::new(
            "arbiter_complete",
            workspace,
            "arbiter marker",
        ));
    }

    if payload
        .get("closure_target_root_spec_id")
        .is_some_and(truthy)
        && is_bool(payload.get("closure_target_open"), false)
    {
        return Some(MonitorEvent::new(
            "arbiter_complete",
            workspace,
            "closure target closed",
        ));
    }

    let execution_marker = truthy_string_or(payload.get("execution_status_marker"), "");
    if notify_terminal_stages
        && execution_marker.trim() == "### UPDATE_COMPLETE"
        && !is_globally_drained(payload)
    {
        return Some(MonitorEvent::new(
            "stage_progress",
            workspace,
            "updater update complete",
        ));
    }

    if is_globally_drained(payload) {
        return Some(MonitorEvent::new(
            "complete",
            workspace,
            "daemon idle with no work",
        ));
    }

    if is_bool(payload.get("process_running"), false)
        && int_value(payload.get("active_run_count")) > 0
    {
        return Some(MonitorEvent::new(
            "crashed",
            workspace,
            "daemon stopped with active runs",
        ));
    }

    None
}

fn has_zero_work(payload: &Map<String, Value>) -> bool {
    [
        "active_run_count",
        "execution_queue_depth",
        "planning_queue_depth",
        "learning_queue_depth",
    ]
    .into_iter()
    .all(|key| int_value(payload.get(key)) == 0)
}

fn is_globally_drained(payload: &Map<String, Value>) -> bool {
    if !has_zero_work(payload) {
        return false;
    }
    if truthy_string_or(payload.get("active_stage"), "none") != "none" {
        return false;
    }
    !is_bool(payload.get("closure_target_open"), true)
}

fn queued_work(payload: &Map<String, Value>) -> i64 {
    [
        "execution_queue_depth",
        "planning_queue_depth",
        "learning_queue_depth",
    ]
    .into_iter()
    .map(|key| int_value(payload.get(key)))
    .sum()
}

fn int_value(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Bool(true)) => 1,
        Some(Value::Bool(false)) | None => 0,
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .unwrap_or(0),
        Some(Value::String(value)) => value.trim().parse::<i64>().unwrap_or(0),
        Some(_) => 0,
    }
}

fn is_bool(value: Option<&Value>, expected: bool) -> bool {
    matches!(value, Some(Value::Bool(value)) if *value == expected)
}

fn truthy_string_or(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(value) if truthy(value) => python_string(value),
        _ => default.to_owned(),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number
            .as_i64()
            .map(|value| value != 0)
            .or_else(|| number.as_u64().map(|value| value != 0))
            .or_else(|| number.as_f64().map(|value| value != 0.0))
            .unwrap_or(false),
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
    }
}

fn python_string(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn duration_from_seconds(seconds: f64) -> Duration {
    if !seconds.is_finite() || seconds <= 0.0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(seconds)
    }
}

fn sleep_seconds(seconds: f64) {
    if seconds.is_finite() && seconds > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(seconds));
    }
}
