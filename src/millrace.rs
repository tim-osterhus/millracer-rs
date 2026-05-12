use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::command::{CommandExecutor, ProcessHandle, SubprocessExecutor, require_success};
use crate::intake::IntakeKind;
use crate::scope::{ScopedWorkItem, render_scoped_work};
use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MillraceConfig {
    pub command: String,
    pub mode: String,
}

impl Default for MillraceConfig {
    fn default() -> Self {
        Self {
            command: "millrace".to_owned(),
            mode: "default_pi".to_owned(),
        }
    }
}

pub struct MillraceController<E = SubprocessExecutor> {
    pub config: MillraceConfig,
    pub executor: E,
    pub workspace: PathBuf,
    pub cwd: Option<PathBuf>,
    daemon: Option<Box<dyn ProcessHandle>>,
}

impl Default for MillraceController<SubprocessExecutor> {
    fn default() -> Self {
        Self {
            config: MillraceConfig::default(),
            executor: SubprocessExecutor,
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            cwd: None,
            daemon: None,
        }
    }
}

impl MillraceController<SubprocessExecutor> {
    pub fn new(config: MillraceConfig, workspace: PathBuf, cwd: Option<PathBuf>) -> Self {
        Self {
            config,
            executor: SubprocessExecutor,
            workspace,
            cwd,
            daemon: None,
        }
    }
}

impl<E> MillraceController<E>
where
    E: CommandExecutor,
{
    pub fn with_executor(
        config: MillraceConfig,
        executor: E,
        workspace: PathBuf,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            config,
            executor,
            workspace,
            cwd,
            daemon: None,
        }
    }

    pub fn set_mode(&mut self, mode: impl Into<String>) {
        self.config.mode = mode.into();
    }

    pub fn initialize(&self) -> MillracerResult<()> {
        self.run_success(vec![
            self.config.command.clone(),
            "init".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
        ])?;
        Ok(())
    }

    pub fn validate(&self) -> MillracerResult<()> {
        self.run_success(vec![
            self.config.command.clone(),
            "compile".to_owned(),
            "validate".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
            "--mode".to_owned(),
            self.config.mode.clone(),
        ])?;
        Ok(())
    }

    pub fn enqueue(
        &self,
        intake_kind: IntakeKind,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        match intake_kind {
            IntakeKind::Probe => self.enqueue_probe(task, scoped_work_item),
            IntakeKind::Idea => self.enqueue_idea(task, scoped_work_item),
            IntakeKind::Task => self.enqueue_task(task, scoped_work_item),
            IntakeKind::Auto => Err(MillracerError::message(
                "unsupported Millrace intake kind: auto",
            )),
        }
    }

    pub fn enqueue_probe(
        &self,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        self.enqueue_intake(IntakeKind::Probe, "add-probe", task, scoped_work_item)
    }

    pub fn enqueue_idea(
        &self,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        self.enqueue_intake(IntakeKind::Idea, "add-idea", task, scoped_work_item)
    }

    pub fn enqueue_task(
        &self,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        self.enqueue_intake(IntakeKind::Task, "add-task", task, scoped_work_item)
    }

    pub fn start_daemon(&mut self) -> MillracerResult<&mut (dyn ProcessHandle + '_)> {
        self.clear_stale_state_if_needed()?;
        let args = vec![
            self.config.command.clone(),
            "run".to_owned(),
            "daemon".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
            "--mode".to_owned(),
            self.config.mode.clone(),
            "--monitor".to_owned(),
            "none".to_owned(),
        ];
        self.daemon = Some(self.executor.start(&args, self.command_cwd(), None)?);
        match self.daemon.as_mut() {
            Some(daemon) => Ok(daemon.as_mut()),
            None => Err(MillracerError::message("millrace daemon was not started")),
        }
    }

    pub fn restart_daemon(&mut self) -> MillracerResult<&mut (dyn ProcessHandle + '_)> {
        self.start_daemon()
    }

    pub fn stop_daemon(&mut self) -> MillracerResult<()> {
        self.run_success(vec![
            self.config.command.clone(),
            "control".to_owned(),
            "stop".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
        ])?;
        self.wait_for_daemon_exit();
        self.clear_stale_state_if_needed()?;
        Ok(())
    }

    pub fn clear_stale_state_if_needed(&self) -> MillracerResult<bool> {
        let Ok(payload) = self.status() else {
            return Ok(false);
        };
        if payload
            .get("runtime_ownership_lock")
            .and_then(Value::as_str)
            != Some("stale")
        {
            return Ok(false);
        }
        self.clear_stale_state()?;
        Ok(true)
    }

    pub fn clear_stale_state(&self) -> MillracerResult<()> {
        self.run_success(vec![
            self.config.command.clone(),
            "control".to_owned(),
            "clear-stale-state".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
        ])?;
        Ok(())
    }

    pub fn status(&self) -> MillracerResult<Map<String, Value>> {
        let result = self.run_success(vec![
            self.config.command.clone(),
            "status".to_owned(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
            "--format".to_owned(),
            "json".to_owned(),
        ])?;
        let stdout = if result.stdout.trim().is_empty() {
            "{}"
        } else {
            result.stdout.trim()
        };
        let Value::Object(payload) = serde_json::from_str::<Value>(stdout)? else {
            return Err(MillracerError::message(
                "millrace status JSON must be an object",
            ));
        };
        Ok(payload)
    }

    fn enqueue_intake(
        &self,
        intake_kind: IntakeKind,
        command: &str,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        let task_path = self.write_intake_file(intake_kind, task, scoped_work_item)?;
        self.run_success(vec![
            self.config.command.clone(),
            "queue".to_owned(),
            command.to_owned(),
            task_path.display().to_string(),
            "--workspace".to_owned(),
            self.workspace.display().to_string(),
        ])?;
        Ok(task_path)
    }

    fn write_intake_file(
        &self,
        intake_kind: IntakeKind,
        task: &str,
        scoped_work_item: Option<&ScopedWorkItem>,
    ) -> MillracerResult<PathBuf> {
        let now = UtcDateTime::now();
        let task_id = task_id(intake_kind, task, now);
        let intake_dir = self.workspace.join(".millracer").join("intake");
        std::fs::create_dir_all(&intake_dir)?;
        let task_path = intake_dir.join(format!("{task_id}.md"));
        let title = title_from_task(task);
        let summary = task.trim();
        let created_at = now.iso8601();
        let raw = match intake_kind {
            IntakeKind::Probe => {
                render_probe_document(&task_id, &title, summary, &created_at, scoped_work_item)
            }
            IntakeKind::Idea => {
                render_idea_document(&task_id, &title, summary, &created_at, scoped_work_item)
            }
            IntakeKind::Task => {
                render_task_document(&task_id, &title, summary, &created_at, scoped_work_item)
            }
            IntakeKind::Auto => {
                return Err(MillracerError::message(
                    "unsupported Millrace intake kind: auto",
                ));
            }
        };
        std::fs::write(&task_path, raw)?;
        Ok(task_path)
    }

    fn wait_for_daemon_exit(&mut self) {
        let Some(daemon) = self.daemon.as_mut() else {
            return;
        };
        if daemon.poll().ok().flatten().is_some() {
            return;
        }
        if daemon.wait(Some(Duration::from_secs(30))).is_ok() {
            return;
        }
        let _ = daemon.terminate();
        if daemon.wait(Some(Duration::from_secs(5))).is_ok() {
            return;
        }
        let _ = daemon.kill();
        let _ = daemon.wait(Some(Duration::from_secs(5)));
    }

    fn run_success(&self, args: Vec<String>) -> MillracerResult<crate::command::CommandResult> {
        require_success(self.executor.run(&args, self.command_cwd(), None, None)?)
            .map_err(|error| MillracerError::message(error.to_string()))
    }

    fn command_cwd(&self) -> &Path {
        self.cwd.as_deref().unwrap_or(self.workspace.as_path())
    }
}

pub fn render_task_document(
    task_id: &str,
    title: &str,
    summary: &str,
    created_at: &str,
    scoped_work_item: Option<&ScopedWorkItem>,
) -> String {
    let scoped_work = render_scoped_work(scoped_work_item);
    format!(
        "\
# {title}

Task-ID: {task_id}
Title: {title}
Summary: {summary}
Created-At: {created_at}
Created-By: millracer

{scoped_work}
Target-Paths:
- .

Acceptance:
- Complete the requested implementation work or report the concrete blocker.
- If this task came from an external queue, complete only the selected scoped work item.

Required-Checks:
- Run the relevant verification for the changed work, or explain why no check was possible.

References:
- queued by Millracer

Scope Contract:
- Treat this document as one scoped work item.
- Do not batch independent queue items into this work item.
- Do not create completion markers, commits, tags, or external signals for any
  work item other than the scoped item named here.
- If the prompt describes a streaming queue but does not identify one selected
  item, report the missing scope instead of processing the whole queue.

Risk:
- This task may need follow-up inspection by the outer Millracer agent.
"
    )
}

pub fn render_probe_document(
    task_id: &str,
    title: &str,
    summary: &str,
    created_at: &str,
    scoped_work_item: Option<&ScopedWorkItem>,
) -> String {
    let scoped_work = render_scoped_work(scoped_work_item);
    format!(
        "\
# {title}

Probe-ID: {task_id}
Title: {title}
Summary: {summary}
Request: {summary}
Created-At: {created_at}
Created-By: millracer

{scoped_work}
Target-Paths:
- .

Constraints:
- Do not implement code changes during this probe stage.
- Do not broaden the selected work item into unrelated implementation.
- Preserve any scoped-work constraints named above.

Recon-Questions:
- Which codebase areas are likely involved?
- Which tests or existing examples define expected behavior?
- Which compatibility and regression risks matter?
- Is this one execution task, or does it need planning/decomposition?
- What should downstream Builder/Checker know before changing code?

Expected-Output:
- Recon packet summarizing findings and candidate files.
- Route recommendation for probe, idea, or task follow-up.
- Downstream work shape with suggested verification.

Acceptance:
- Produce a recon packet, route recommendation, and downstream work shape.

Risk-Notes:
- Acting before reconnaissance may miss repository conventions or regression risks.

References:
- queued by Millracer
"
    )
}

pub fn render_idea_document(
    task_id: &str,
    title: &str,
    summary: &str,
    created_at: &str,
    scoped_work_item: Option<&ScopedWorkItem>,
) -> String {
    let scoped_work = render_scoped_work(scoped_work_item);
    format!(
        "\
# {title}

Idea-ID: {task_id}
Title: {title}
Desired-Outcome: {summary}
Created-At: {created_at}
Created-By: millracer

{scoped_work}
Operator-Visible-Value:
- Preserve the requested outcome and make the work actionable.

Constraints:
- Preserve any scoped-work constraints named above.
- Do not expand into unrelated work items.

Acceptance-Intent:
- Shape the idea into clear implementation slices and verification expectations.
- Identify blockers or missing decisions before execution.

Planning-Intent:
- Decompose only as needed to make downstream execution safe and reviewable.
- Recommend the appropriate execution mode or follow-up intake kind.

References:
- queued by Millracer
"
    )
}

fn task_id(intake_kind: IntakeKind, task: &str, now: UtcDateTime) -> String {
    format!(
        "{}-{}-{}",
        intake_kind.as_str(),
        now.compact(),
        slug(task).chars().take(32).collect::<String>()
    )
}

fn title_from_task(task: &str) -> String {
    let first_line = task.trim().lines().next().unwrap_or("Millracer task");
    let title = first_line.chars().take(80).collect::<String>();
    let title = title.trim();
    if title.is_empty() {
        "Millracer task".to_owned()
    } else {
        title.to_owned()
    }
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch);
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug
    }
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
    fn now() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::from_unix_seconds(seconds)
    }

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

    fn compact(self) -> String {
        format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
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
