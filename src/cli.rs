use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::agent::{AgentSession, MillracerAgent, ProductionPi, RunOptions};
use crate::benchmark::{RunResult, parse_benchmark_request, render_benchmark_result};
use crate::command::SubprocessExecutor;
use crate::intake::IntakeKind;
use crate::millrace::{MillraceConfig, MillraceController};
use crate::monitor::DaemonMonitor;
use crate::operator::MillracerOperator;
use crate::pi::{PiConfig, PiHarness, discover_default_skill_paths, normalize_path};
use crate::pi_rpc::PiRpcHarness;
use crate::scope::ScopedWorkItem;
use crate::{MillracerError, MillracerResult};

type StatusLoader = Box<dyn FnMut() -> MillracerResult<Map<String, Value>>>;
type ProductionMonitor = DaemonMonitor<StatusLoader>;
type ProductionAgent =
    MillracerAgent<ProductionPi, MillraceController<SubprocessExecutor>, ProductionMonitor>;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "millracer",
    version,
    about = "Pi-backed Millrace-aware operator harness."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Run one Millracer task.
    Run(RunArgs),
    /// Start a persistent Millracer operator.
    Operator(OperatorArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// Task text. Reads stdin when omitted.
    pub task: Vec<String>,
    /// Read external request JSON from stdin.
    #[arg(long = "benchmark-json", action = ArgAction::SetTrue)]
    pub benchmark_json: bool,
    #[command(flatten)]
    pub common: CommonOptions,
    #[arg(long = "pi-session", value_enum, default_value_t = PiSession::Rpc)]
    pub pi_session: PiSession,
    #[arg(long = "output", value_enum, default_value_t = OutputMode::Text)]
    pub output: OutputMode,
}

#[derive(Debug, Clone, Args)]
pub struct OperatorArgs {
    #[command(flatten)]
    pub common: CommonOptions,
}

#[derive(Debug, Clone, Args)]
pub struct CommonOptions {
    #[arg(long = "workspace", default_value = ".")]
    pub workspace: PathBuf,
    #[arg(long = "cwd")]
    pub cwd: Option<PathBuf>,
    #[arg(long = "route", value_enum, default_value_t = Route::Auto)]
    pub route: Route,
    #[arg(long = "intake", value_enum, default_value_t = Intake::Auto)]
    pub intake: Intake,
    #[arg(long = "pi-command", default_value = "pi")]
    pub pi_command: String,
    #[arg(long = "millrace-command", default_value = "millrace")]
    pub millrace_command: String,
    #[arg(long = "provider")]
    pub provider: Option<String>,
    #[arg(long = "model")]
    pub model: Option<String>,
    #[arg(long = "thinking", default_value = "high")]
    pub thinking: String,
    #[arg(long = "millrace-mode", default_value = "default_pi")]
    pub millrace_mode: String,
    #[arg(long = "skill", value_name = "PATH")]
    pub skill: Vec<PathBuf>,
    #[arg(long = "no-default-skills", action = ArgAction::SetTrue)]
    pub no_default_skills: bool,
    #[arg(long = "poll-interval-seconds", default_value_t = 2.0)]
    pub poll_interval_seconds: f64,
    #[arg(long = "daemon-timeout-seconds", default_value_t = 7200.0)]
    pub daemon_timeout_seconds: f64,
    #[arg(long = "max-daemon-restarts", default_value_t = 1)]
    pub max_daemon_restarts: i32,
    #[arg(long = "pi-timeout-seconds")]
    pub pi_timeout_seconds: Option<i32>,
    #[arg(long = "keep-daemon", action = ArgAction::SetTrue)]
    pub keep_daemon: bool,
    #[arg(long = "notify-terminal-stages", action = ArgAction::SetTrue)]
    pub notify_terminal_stages: bool,
    #[arg(long = "no-notify-terminal-stages", action = ArgAction::SetTrue)]
    pub no_notify_terminal_stages: bool,
}

impl CommonOptions {
    pub fn notify_terminal_stages(&self) -> bool {
        !self.no_notify_terminal_stages
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Auto,
    Direct,
    Millrace,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Direct => "direct",
            Self::Millrace => "millrace",
        }
    }
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Intake {
    Auto,
    Probe,
    Idea,
    Task,
}

impl Intake {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Probe => "probe",
            Self::Idea => "idea",
            Self::Task => "task",
        }
    }
}

impl std::fmt::Display for Intake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PiSession {
    Rpc,
    Print,
}

impl PiSession {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rpc => "rpc",
            Self::Print => "print",
        }
    }
}

impl std::fmt::Display for PiSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliOutcome {
    fn success(stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            stdout: String::new(),
            stderr: format!("millracer: error: {}\n", message.into()),
        }
    }
}

pub fn main() -> i32 {
    let args = std::env::args_os().collect::<Vec<OsString>>();
    let parsed = match parse_cli(args.clone()) {
        Ok(cli) => cli,
        Err(outcome) => return write_outcome(outcome),
    };

    match parsed.command.clone() {
        Some(Commands::Operator(operator_args)) if io::stdin().is_terminal() => {
            run_operator_interactive(operator_args)
        }
        Some(Commands::Run(run_args)) if run_args.needs_stdin() => {
            let mut stdin = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut stdin) {
                return write_outcome(CliOutcome::error(error.to_string()));
            }
            write_outcome(dispatch(parsed, &stdin))
        }
        Some(Commands::Operator(_)) => {
            let mut stdin = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut stdin) {
                return write_outcome(CliOutcome::error(error.to_string()));
            }
            write_outcome(dispatch(parsed, &stdin))
        }
        _ => write_outcome(dispatch(parsed, "")),
    }
}

pub fn run_from<I, T>(args: I, stdin: &str) -> CliOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match parse_cli(args) {
        Ok(cli) => dispatch(cli, stdin),
        Err(outcome) => outcome,
    }
}

fn parse_cli<I, T>(args: I) -> Result<Cli, CliOutcome>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    Cli::try_parse_from(args).map_err(|error| {
        let exit_code = error.exit_code();
        let output = error.to_string();
        match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => CliOutcome {
                exit_code,
                stdout: output,
                stderr: String::new(),
            },
            _ => CliOutcome {
                exit_code,
                stdout: String::new(),
                stderr: output,
            },
        }
    })
}

fn dispatch(cli: Cli, stdin: &str) -> CliOutcome {
    match cli.command {
        Some(Commands::Run(args)) => handle_run(args, stdin),
        Some(Commands::Operator(args)) => handle_operator(args, stdin),
        None => {
            let mut help = Vec::new();
            let mut command = Cli::command();
            let _ = command.write_long_help(&mut help);
            CliOutcome {
                exit_code: 2,
                stdout: String::from_utf8_lossy(&help).into_owned(),
                stderr: String::new(),
            }
        }
    }
}

fn handle_run(args: RunArgs, stdin: &str) -> CliOutcome {
    match build_run_result(&args, stdin) {
        Ok(result) => match args.output {
            OutputMode::Text => CliOutcome::success(
                format!("{}\n", result.output),
                warning_stderr(&result.warnings),
            ),
            OutputMode::Json => match render_benchmark_result(&result) {
                Ok(json) => CliOutcome::success(format!("{json}\n"), String::new()),
                Err(error) => CliOutcome::error(error.to_string()),
            },
        },
        Err(error) => CliOutcome::error(error.to_string()),
    }
}

fn handle_operator(args: OperatorArgs, stdin: &str) -> CliOutcome {
    let mut operator = match build_production_operator(&args) {
        Ok(operator) => operator,
        Err(error) => return CliOutcome::error(error.to_string()),
    };
    let mut stdout = String::new();
    let mut stderr = "Millracer operator ready. Type /exit to quit.\n".to_owned();
    for raw in stdin.lines() {
        let task = raw.trim();
        if task == "/exit" || task == "/quit" {
            break;
        }
        if task.is_empty() {
            continue;
        }
        match operator.handle(task) {
            Ok(result) => {
                stderr.push_str(&warning_stderr(&result.warnings));
                stdout.push_str(&result.output);
                stdout.push('\n');
            }
            Err(error) => {
                stderr.push_str(&format!("millracer: error: {error}\n"));
            }
        }
    }
    if let Err(error) = operator.close() {
        stderr.push_str(&format!("millracer: error: {error}\n"));
        return CliOutcome {
            exit_code: 1,
            stdout,
            stderr,
        };
    }
    CliOutcome::success(stdout, stderr)
}

fn run_operator_interactive(args: OperatorArgs) -> i32 {
    let mut operator = match build_production_operator(&args) {
        Ok(operator) => operator,
        Err(error) => {
            eprintln!("millracer: error: {error}");
            return 1;
        }
    };
    eprintln!("Millracer operator ready. Type /exit to quit.");
    let mut line = String::new();
    let mut exit_code = 0;
    'prompt: loop {
        eprint!("millracer> ");
        let _ = io::stderr().flush();
        line.clear();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let task = line.trim();
                if task == "/exit" || task == "/quit" {
                    break;
                }
                if task.is_empty() {
                    continue;
                }
                match operator.handle(task) {
                    Ok(result) => {
                        for warning in &result.warnings {
                            eprintln!("millracer: warning: {warning}");
                        }
                        println!("{}", result.output);
                    }
                    Err(error) => eprintln!("millracer: error: {error}"),
                }
            }
            Err(error) => {
                eprintln!("millracer: error: {error}");
                exit_code = 1;
                break 'prompt;
            }
        }
    }
    if let Err(error) = operator.close() {
        eprintln!("millracer: error: {error}");
        return 1;
    }
    exit_code
}

fn build_run_result(args: &RunArgs, stdin: &str) -> MillracerResult<RunResult> {
    let (task, workspace, scoped_work_item, request_intake) =
        task_workspace_and_scope(args, stdin)?;
    let workspace = normalize_path(&workspace);
    let cwd = args
        .common
        .cwd
        .as_ref()
        .map(|cwd| normalize_path(cwd))
        .unwrap_or_else(|| workspace.clone());
    let requested_intake = requested_intake(args.common.intake, request_intake);
    let mut agent = build_production_agent(&args.common, args.pi_session, &workspace, &cwd);
    let result = agent.run(
        &task,
        RunOptions {
            workspace,
            cwd,
            route: args.common.route.to_string(),
            daemon_timeout_seconds: args.common.daemon_timeout_seconds,
            pi_timeout_seconds: args.common.pi_timeout_seconds,
            keep_daemon: args.common.keep_daemon,
            scoped_work_item,
            max_daemon_restarts: args.common.max_daemon_restarts,
            intake: requested_intake.to_string(),
            notify_terminal_stages: args.common.notify_terminal_stages(),
            pi_session: args.pi_session.to_string(),
            millrace_mode: args.common.millrace_mode.clone(),
        },
    );
    let close_result = agent.close();
    match (result, close_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn task_workspace_and_scope(
    args: &RunArgs,
    stdin: &str,
) -> MillracerResult<(String, PathBuf, Option<ScopedWorkItem>, Option<IntakeKind>)> {
    if args.benchmark_json {
        let request = parse_benchmark_request(stdin)?;
        let request_intake = request
            .intake_kind
            .as_deref()
            .map(intake_from_str)
            .transpose()?;
        let workspace = request
            .workspace
            .clone()
            .unwrap_or_else(|| args.common.workspace.clone());
        return Ok((
            request.task,
            workspace,
            request.scoped_work_item,
            request_intake,
        ));
    }

    let task = args.task.join(" ").trim().to_owned();
    let task = if task.is_empty() {
        stdin.trim().to_owned()
    } else {
        task
    };
    if task.is_empty() {
        return Err(MillracerError::message("task text is required"));
    }
    Ok((task, args.common.workspace.clone(), None, None))
}

fn intake_from_str(value: &str) -> MillracerResult<IntakeKind> {
    match value {
        "probe" => Ok(IntakeKind::Probe),
        "idea" => Ok(IntakeKind::Idea),
        "task" => Ok(IntakeKind::Task),
        _ => Err(MillracerError::message(format!(
            "invalid intake_kind `{value}`; expected probe, idea, or task"
        ))),
    }
}

fn intake_kind_from_cli(value: Intake) -> IntakeKind {
    match value {
        Intake::Auto => IntakeKind::Auto,
        Intake::Probe => IntakeKind::Probe,
        Intake::Idea => IntakeKind::Idea,
        Intake::Task => IntakeKind::Task,
    }
}

fn requested_intake(cli_intake: Intake, request_intake: Option<IntakeKind>) -> IntakeKind {
    if cli_intake == Intake::Auto {
        request_intake.unwrap_or(IntakeKind::Auto)
    } else {
        intake_kind_from_cli(cli_intake)
    }
}

fn build_production_operator(
    args: &OperatorArgs,
) -> MillracerResult<MillracerOperator<ProductionAgent>> {
    let workspace = normalize_path(&args.common.workspace);
    let cwd = args
        .common
        .cwd
        .as_ref()
        .map(|cwd| normalize_path(cwd))
        .unwrap_or_else(|| workspace.clone());
    let agent = build_production_agent(&args.common, PiSession::Rpc, &workspace, &cwd);
    Ok(MillracerOperator {
        agent,
        workspace,
        cwd,
        route: args.common.route.to_string(),
        daemon_timeout_seconds: args.common.daemon_timeout_seconds,
        pi_timeout_seconds: args.common.pi_timeout_seconds,
        keep_daemon: args.common.keep_daemon,
        max_daemon_restarts: args.common.max_daemon_restarts,
        intake: intake_kind_from_cli(args.common.intake).to_string(),
        notify_terminal_stages: args.common.notify_terminal_stages(),
        pi_session: PiSession::Rpc.to_string(),
        millrace_mode: args.common.millrace_mode.clone(),
    })
}

fn build_production_agent(
    common: &CommonOptions,
    pi_session: PiSession,
    workspace: &Path,
    cwd: &Path,
) -> ProductionAgent {
    let skill_paths = if common.skill.is_empty() && !common.no_default_skills {
        discover_default_skill_paths(None)
    } else {
        common
            .skill
            .iter()
            .map(|path| normalize_path(path))
            .collect()
    };
    let pi_config = PiConfig {
        command: common.pi_command.clone(),
        provider: common.provider.clone(),
        model: common.model.clone(),
        thinking: Some(common.thinking.clone()).filter(|value| !value.is_empty()),
        skill_paths,
        extra_args: Vec::new(),
    };
    let pi = match pi_session {
        PiSession::Print => ProductionPi::Print(PiHarness::new(pi_config)),
        PiSession::Rpc => ProductionPi::Rpc(PiRpcHarness::new(pi_config)),
    };
    let millrace_config = MillraceConfig {
        command: common.millrace_command.clone(),
        mode: common.millrace_mode.clone(),
    };
    let millrace = MillraceController::new(
        millrace_config.clone(),
        workspace.to_path_buf(),
        Some(cwd.to_path_buf()),
    );
    let status_controller = MillraceController::new(
        millrace_config,
        workspace.to_path_buf(),
        Some(cwd.to_path_buf()),
    );
    let status_loader: StatusLoader = Box::new(move || status_controller.status());
    let monitor = DaemonMonitor::with_poll_interval(status_loader, common.poll_interval_seconds)
        .with_notify_terminal_stages(common.notify_terminal_stages());
    MillracerAgent::new(pi, millrace, monitor)
}

fn warning_stderr(warnings: &[String]) -> String {
    warnings
        .iter()
        .map(|warning| format!("millracer: warning: {warning}\n"))
        .collect()
}

fn write_outcome(outcome: CliOutcome) -> i32 {
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        eprint!("{}", outcome.stderr);
    }
    outcome.exit_code
}

impl RunArgs {
    fn needs_stdin(&self) -> bool {
        self.benchmark_json || self.task.is_empty()
    }
}
