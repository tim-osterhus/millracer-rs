use std::path::PathBuf;

use crate::MillracerResult;
use crate::agent::{AgentSession, RunOptions};
use crate::benchmark::RunResult;

pub struct MillracerOperator<A> {
    pub agent: A,
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    pub route: String,
    pub daemon_timeout_seconds: f64,
    pub pi_timeout_seconds: Option<i32>,
    pub keep_daemon: bool,
    pub max_daemon_restarts: i32,
    pub intake: String,
    pub notify_terminal_stages: bool,
    pub pi_session: String,
    pub millrace_mode: String,
}

impl<A> MillracerOperator<A>
where
    A: AgentSession,
{
    pub fn handle(&mut self, task: &str) -> MillracerResult<RunResult> {
        self.agent.run(
            task,
            RunOptions {
                workspace: self.workspace.clone(),
                cwd: self.cwd.clone(),
                route: self.route.clone(),
                daemon_timeout_seconds: self.daemon_timeout_seconds,
                pi_timeout_seconds: self.pi_timeout_seconds,
                keep_daemon: self.keep_daemon,
                scoped_work_item: None,
                max_daemon_restarts: self.max_daemon_restarts,
                intake: self.intake.clone(),
                notify_terminal_stages: self.notify_terminal_stages,
                pi_session: self.pi_session.clone(),
                millrace_mode: self.millrace_mode.clone(),
            },
        )
    }

    pub fn close(&mut self) -> MillracerResult<()> {
        self.agent.close()
    }
}
