use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use crate::MillracerResult;
use crate::benchmark::RunResult;
use crate::command::{CommandExecutor, SubprocessExecutor};
use crate::decision::{Decision, parse_decision};
use crate::intake::{IntakeKind, choose_intake_kind, normalize_intake_kind};
use crate::millrace::MillraceController;
use crate::monitor::{DaemonMonitor, MonitorEvent};
use crate::pi::PiHarness;
use crate::pi_rpc::PiRpcHarness;
use crate::prompts::{
    FinalizationPrompt, decision_prompt, direct_prompt, finalization_prompt, progress_prompt,
};
use crate::scope::{ScopedWorkItem, scoped_work_json};

#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    pub route: String,
    pub daemon_timeout_seconds: f64,
    pub pi_timeout_seconds: Option<i32>,
    pub keep_daemon: bool,
    pub scoped_work_item: Option<ScopedWorkItem>,
    pub max_daemon_restarts: i32,
    pub intake: String,
    pub notify_terminal_stages: bool,
    pub pi_session: String,
    pub millrace_mode: String,
}

impl RunOptions {
    pub fn new(workspace: PathBuf, cwd: PathBuf) -> Self {
        Self {
            workspace,
            cwd,
            route: "auto".to_owned(),
            daemon_timeout_seconds: 7200.0,
            pi_timeout_seconds: None,
            keep_daemon: false,
            scoped_work_item: None,
            max_daemon_restarts: 1,
            intake: "auto".to_owned(),
            notify_terminal_stages: true,
            pi_session: "rpc".to_owned(),
            millrace_mode: "default_pi".to_owned(),
        }
    }
}

pub trait PiLike {
    fn complete(
        &mut self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String>;

    fn close(&mut self) -> MillracerResult<()> {
        Ok(())
    }
}

impl<E> PiLike for PiHarness<E>
where
    E: CommandExecutor,
{
    fn complete(
        &mut self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String> {
        PiHarness::complete(self, prompt, cwd, timeout)
    }
}

impl PiLike for PiRpcHarness {
    fn complete(
        &mut self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String> {
        PiRpcHarness::complete(self, prompt, cwd, timeout)
    }

    fn close(&mut self) -> MillracerResult<()> {
        PiRpcHarness::close(self)
    }
}

pub trait MillraceLike {
    fn set_mode(&mut self, mode: &str) -> MillracerResult<()> {
        let _ = mode;
        Ok(())
    }

    fn initialize(&mut self) -> MillracerResult<()>;
    fn validate(&mut self) -> MillracerResult<()>;
    fn enqueue(
        &mut self,
        intake_kind: IntakeKind,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf>;
    fn find_existing_scoped_intake(
        &mut self,
        scoped_work_item: &ScopedWorkItem,
    ) -> MillracerResult<Option<PathBuf>> {
        let _ = scoped_work_item;
        Ok(None)
    }
    fn start_daemon(&mut self) -> MillracerResult<()>;
    fn restart_daemon(&mut self) -> MillracerResult<()>;
    fn stop_daemon(&mut self) -> MillracerResult<()>;
    fn status(&mut self) -> MillracerResult<Map<String, Value>>;
}

impl<E> MillraceLike for MillraceController<E>
where
    E: CommandExecutor,
{
    fn set_mode(&mut self, mode: &str) -> MillracerResult<()> {
        MillraceController::set_mode(self, mode.to_owned());
        Ok(())
    }

    fn initialize(&mut self) -> MillracerResult<()> {
        MillraceController::initialize(self)
    }

    fn validate(&mut self) -> MillracerResult<()> {
        MillraceController::validate(self)
    }

    fn enqueue(
        &mut self,
        intake_kind: IntakeKind,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        MillraceController::enqueue(self, intake_kind, task, scoped_work_item)
    }

    fn find_existing_scoped_intake(
        &mut self,
        scoped_work_item: &ScopedWorkItem,
    ) -> MillracerResult<Option<PathBuf>> {
        MillraceController::find_existing_scoped_intake(self, scoped_work_item)
    }

    fn start_daemon(&mut self) -> MillracerResult<()> {
        MillraceController::start_daemon(self)?;
        Ok(())
    }

    fn restart_daemon(&mut self) -> MillracerResult<()> {
        MillraceController::restart_daemon(self)?;
        Ok(())
    }

    fn stop_daemon(&mut self) -> MillracerResult<()> {
        MillraceController::stop_daemon(self)
    }

    fn status(&mut self) -> MillracerResult<Map<String, Value>> {
        MillraceController::status(self)
    }
}

pub trait MonitorLike {
    fn wait(&mut self, timeout_seconds: f64) -> MillracerResult<MonitorEvent>;
}

impl<L, S> MonitorLike for DaemonMonitor<L, S>
where
    L: FnMut() -> MillracerResult<Map<String, Value>>,
    S: FnMut(f64),
{
    fn wait(&mut self, timeout_seconds: f64) -> MillracerResult<MonitorEvent> {
        DaemonMonitor::wait(self, timeout_seconds)
    }
}

pub trait AgentSession {
    fn run(&mut self, task: &str, options: RunOptions) -> MillracerResult<RunResult>;

    fn close(&mut self) -> MillracerResult<()> {
        Ok(())
    }
}

pub struct MillracerAgent<P, M, O> {
    pub pi: P,
    pub millrace: M,
    pub monitor: O,
}

impl<P, M, O> MillracerAgent<P, M, O>
where
    P: PiLike,
    M: MillraceLike,
    O: MonitorLike,
{
    pub fn new(pi: P, millrace: M, monitor: O) -> Self {
        Self {
            pi,
            millrace,
            monitor,
        }
    }

    fn decision_for(&mut self, task: &str, options: &RunOptions) -> MillracerResult<Decision> {
        let route = options.route.trim().to_ascii_lowercase();
        if route == "direct" || route == "millrace" {
            let mut decision =
                Decision::new(route.clone(), format!("route forced by --route {route}"));
            decision.mode = options.millrace_mode.clone();
            return Ok(decision);
        }

        let raw_decision = self.pi.complete(
            &decision_prompt(task),
            &options.cwd,
            timeout_from_seconds(options.pi_timeout_seconds),
        )?;
        Ok(parse_decision(&raw_decision))
    }

    fn wait_for_terminal_event(
        &mut self,
        task: &str,
        intake_kind: &str,
        options: &RunOptions,
    ) -> MillracerResult<(MonitorEvent, Vec<MonitorEvent>)> {
        let mut restarts = 0;
        let mut progress_events = Vec::new();
        loop {
            let event = self.monitor.wait(options.daemon_timeout_seconds)?;
            if event.kind == "stage_progress" {
                if options.notify_terminal_stages {
                    progress_events.push(event.clone());
                    self.pi.complete(
                        &progress_prompt(
                            task,
                            &options.workspace.display().to_string(),
                            intake_kind,
                            &event.kind,
                            &event.reason,
                        ),
                        &options.cwd,
                        timeout_from_seconds(options.pi_timeout_seconds),
                    )?;
                }
                continue;
            }

            if event.kind != "restart_needed" {
                return Ok((event, progress_events));
            }
            if restarts >= options.max_daemon_restarts {
                return Ok((event, progress_events));
            }
            self.millrace.restart_daemon()?;
            restarts += 1;
        }
    }
}

impl<P, M, O> AgentSession for MillracerAgent<P, M, O>
where
    P: PiLike,
    M: MillraceLike,
    O: MonitorLike,
{
    fn run(&mut self, task: &str, options: RunOptions) -> MillracerResult<RunResult> {
        let decision = self.decision_for(task, &options)?;
        let requested_intake =
            normalize_intake_kind(&options.intake, true).unwrap_or(IntakeKind::Auto);
        let intake_decision = choose_intake_kind(task, requested_intake, Some(&decision));
        let intake_kind = intake_decision.intake_kind;
        let intake_kind_text = intake_kind.as_str().to_owned();
        let warnings = warnings_for_decision(&decision);

        if decision.route == "direct" {
            let output = self.pi.complete(
                &direct_prompt(task),
                &options.cwd,
                timeout_from_seconds(options.pi_timeout_seconds),
            )?;
            let mut result = base_result(
                BaseResultInput {
                    route: "direct",
                    decision,
                    output,
                    intake_kind: intake_kind_text,
                    intake_signals: intake_decision.signals,
                    warnings,
                },
                task,
                &options,
            );
            result.outcome = "completed".to_owned();
            return Ok(result);
        }

        self.millrace.set_mode(&decision.mode)?;
        self.millrace.initialize()?;
        self.millrace.validate()?;
        let existing_scoped_intake =
            if let Some(scoped_work_item) = options.scoped_work_item.as_ref() {
                self.millrace
                    .find_existing_scoped_intake(scoped_work_item)?
            } else {
                None
            };
        if let Some(task_path) = existing_scoped_intake.as_ref() {
            let status = self.millrace.status()?;
            if let Some(result) = existing_blocked_scoped_result(
                ExistingBlockedInput {
                    decision: decision.clone(),
                    intake_kind: intake_kind_text.clone(),
                    intake_signals: intake_decision.signals.clone(),
                    warnings: warnings.clone(),
                    task_path,
                    status,
                },
                task,
                &options,
                &mut self.pi,
            )? {
                return Ok(result);
            }
        }
        let task_path = if let Some(task_path) = existing_scoped_intake {
            task_path
        } else if let Some(scoped_work_item) = options.scoped_work_item.as_ref() {
            self.millrace
                .enqueue(intake_kind, task, Some(scoped_work_item))?
        } else {
            self.millrace.enqueue(intake_kind, task, None)?
        };
        self.millrace.start_daemon()?;
        let (event, progress_events) =
            self.wait_for_terminal_event(task, &intake_kind_text, &options)?;
        if !options.keep_daemon {
            self.millrace.stop_daemon()?;
        }
        let status = self.millrace.status()?;
        let status_value = Value::Object(status.clone());
        let status_json = serde_json::to_string_pretty(&status_value)?;
        let progress_json = serde_json::to_string_pretty(&progress_events)?;
        let scoped_json = scoped_work_json(options.scoped_work_item.as_ref());
        let (outcome, scoped_completion, completion_evidence) = outcome_for_event(&event);
        let completion_evidence_json = serde_json::to_string_pretty(&completion_evidence)?;
        let output = self.pi.complete(
            &finalization_prompt(FinalizationPrompt {
                task,
                workspace: &options.workspace.display().to_string(),
                route: "millrace",
                intake_kind: &intake_kind_text,
                outcome: &outcome,
                scoped_completion,
                completion_evidence_json: &completion_evidence_json,
                event_kind: &event.kind,
                event_reason: &event.reason,
                status_json: &status_json,
                warnings: &warnings,
                scoped_work_json: scoped_json.as_deref(),
                progress_events_json: Some(&progress_json),
            }),
            &options.cwd,
            timeout_from_seconds(options.pi_timeout_seconds),
        )?;

        let mut result = base_result(
            BaseResultInput {
                route: "millrace",
                decision,
                output,
                intake_kind: intake_kind_text,
                intake_signals: intake_decision.signals,
                warnings,
            },
            task,
            &options,
        );
        result.event = Some(event);
        result.task_path = Some(task_path.display().to_string());
        result.status = Some(status_value);
        result.progress_events = progress_events;
        result.outcome = outcome;
        result.scoped_completion = scoped_completion;
        result.completion_evidence = completion_evidence;
        Ok(result)
    }

    fn close(&mut self) -> MillracerResult<()> {
        self.pi.close()
    }
}

pub enum ProductionPi {
    Print(PiHarness<SubprocessExecutor>),
    Rpc(PiRpcHarness),
}

impl PiLike for ProductionPi {
    fn complete(
        &mut self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String> {
        match self {
            Self::Print(pi) => pi.complete(prompt, cwd, timeout),
            Self::Rpc(pi) => pi.complete(prompt, cwd, timeout),
        }
    }

    fn close(&mut self) -> MillracerResult<()> {
        match self {
            Self::Print(_) => Ok(()),
            Self::Rpc(pi) => pi.close(),
        }
    }
}

pub fn warnings_for_decision(decision: &Decision) -> Vec<String> {
    if !decision.custom_loop_needed {
        return Vec::new();
    }
    vec![format!(
        "Pi indicated that a custom Millrace loop may be needed; Millracer will still use `{}` unless the caller passes a different --millrace-mode.",
        decision.mode
    )]
}

fn base_result(input: BaseResultInput, task: &str, options: &RunOptions) -> RunResult {
    RunResult {
        route: input.route.to_owned(),
        intake_kind: input.intake_kind,
        intake_signals: input.intake_signals,
        decision: input.decision,
        output: input.output,
        event: None,
        task_path: None,
        status: None,
        warnings: input.warnings,
        outcome: "incomplete".to_owned(),
        scoped_completion: false,
        completion_evidence: Vec::new(),
        scoped_work_item: options.scoped_work_item.clone(),
        progress_events: Vec::new(),
        task: task.to_owned(),
        workspace: options.workspace.display().to_string(),
        cwd: options.cwd.display().to_string(),
        pi_session: options.pi_session.clone(),
        millrace_mode: options.millrace_mode.clone(),
        notify_terminal_stages: options.notify_terminal_stages,
    }
}

struct BaseResultInput {
    route: &'static str,
    decision: Decision,
    output: String,
    intake_kind: String,
    intake_signals: Vec<String>,
    warnings: Vec<String>,
}

struct ExistingBlockedInput<'a> {
    decision: Decision,
    intake_kind: String,
    intake_signals: Vec<String>,
    warnings: Vec<String>,
    task_path: &'a Path,
    status: Map<String, Value>,
}

fn existing_blocked_scoped_result<P>(
    input: ExistingBlockedInput<'_>,
    task: &str,
    options: &RunOptions,
    pi: &mut P,
) -> MillracerResult<Option<RunResult>>
where
    P: PiLike,
{
    let failure_class = input.status.get("current_failure_class");
    let latest_error = input.status.get("latest_runtime_error_report_path");
    if !json_truthy(failure_class) && !json_truthy(latest_error) {
        return Ok(None);
    }

    let reason = failure_class
        .filter(|value| json_truthy(Some(value)))
        .map(json_text)
        .unwrap_or_else(|| "latest runtime error".to_owned());
    let workspace = truthy_text_or(
        input.status.get("workspace"),
        &options.workspace.display().to_string(),
    );
    let event = MonitorEvent::new("blocked", workspace, reason);
    let status_value = Value::Object(input.status.clone());
    let status_json = serde_json::to_string_pretty(&status_value)?;
    let scoped_json = scoped_work_json(options.scoped_work_item.as_ref());
    let completion_evidence_json = "[]";
    let progress_events_json = "[]";
    let output = pi.complete(
        &finalization_prompt(FinalizationPrompt {
            task,
            workspace: &options.workspace.display().to_string(),
            route: "millrace",
            intake_kind: &input.intake_kind,
            outcome: "blocked",
            scoped_completion: false,
            completion_evidence_json,
            event_kind: &event.kind,
            event_reason: &event.reason,
            status_json: &status_json,
            warnings: &input.warnings,
            scoped_work_json: scoped_json.as_deref(),
            progress_events_json: Some(progress_events_json),
        }),
        &options.cwd,
        timeout_from_seconds(options.pi_timeout_seconds),
    )?;

    let mut result = base_result(
        BaseResultInput {
            route: "millrace",
            decision: input.decision,
            output,
            intake_kind: input.intake_kind,
            intake_signals: input.intake_signals,
            warnings: input.warnings,
        },
        task,
        options,
    );
    result.event = Some(event);
    result.task_path = Some(input.task_path.display().to_string());
    result.status = Some(status_value);
    result.outcome = "blocked".to_owned();
    Ok(Some(result))
}

fn outcome_for_event(event: &MonitorEvent) -> (String, bool, Vec<BTreeMap<String, String>>) {
    if event.kind == "arbiter_complete" || event.kind == "scoped_complete" {
        return (
            "completed".to_owned(),
            true,
            vec![BTreeMap::from([
                ("kind".to_owned(), event.kind.clone()),
                ("reason".to_owned(), event.reason.clone()),
                ("workspace".to_owned(), event.workspace.clone()),
            ])],
        );
    }

    let outcome = match event.kind.as_str() {
        "blocked" => "blocked",
        "restart_needed" => "restart_needed",
        "crashed" => "crashed",
        _ => "incomplete",
    };
    (outcome.to_owned(), false, Vec::new())
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

fn json_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn truthy_text_or(value: Option<&Value>, fallback: &str) -> String {
    value
        .filter(|item| json_truthy(Some(item)))
        .map(json_text)
        .unwrap_or_else(|| fallback.to_owned())
}

fn timeout_from_seconds(seconds: Option<i32>) -> Option<Duration> {
    let seconds = seconds?;
    let seconds = u64::try_from(seconds).ok()?;
    Some(Duration::from_secs(seconds))
}
