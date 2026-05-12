use std::path::PathBuf;

use millracer::agent::{AgentSession, RunOptions};
use millracer::benchmark::RunResult;
use millracer::decision::Decision;
use millracer::operator::MillracerOperator;

#[derive(Debug, Default)]
struct FakeAgent {
    tasks: Vec<String>,
    closed: bool,
}

impl AgentSession for FakeAgent {
    fn run(&mut self, task: &str, _options: RunOptions) -> millracer::MillracerResult<RunResult> {
        self.tasks.push(task.to_owned());
        Ok(RunResult {
            route: "direct".to_owned(),
            intake_kind: "probe".to_owned(),
            intake_signals: Vec::new(),
            decision: Decision::new("direct", "test"),
            output: task.to_ascii_uppercase(),
            event: None,
            task_path: None,
            status: None,
            warnings: Vec::new(),
            outcome: "completed".to_owned(),
            scoped_completion: false,
            completion_evidence: Vec::new(),
            scoped_work_item: None,
            progress_events: Vec::new(),
            task: task.to_owned(),
            workspace: "/tmp/ws".to_owned(),
            cwd: "/tmp/ws".to_owned(),
            pi_session: "rpc".to_owned(),
            millrace_mode: "default_pi".to_owned(),
            notify_terminal_stages: true,
        })
    }

    fn close(&mut self) -> millracer::MillracerResult<()> {
        self.closed = true;
        Ok(())
    }
}

#[test]
fn operator_keeps_one_agent_session_across_tasks() {
    let mut operator = MillracerOperator {
        agent: FakeAgent::default(),
        workspace: PathBuf::from("/tmp/ws"),
        cwd: PathBuf::from("/tmp/ws"),
        route: "auto".to_owned(),
        daemon_timeout_seconds: 7200.0,
        pi_timeout_seconds: None,
        keep_daemon: false,
        max_daemon_restarts: 1,
        intake: "auto".to_owned(),
        notify_terminal_stages: true,
        pi_session: "rpc".to_owned(),
        millrace_mode: "default_pi".to_owned(),
    };

    let first = operator.handle("first task").expect("first result");
    let second = operator.handle("second task").expect("second result");
    operator.close().expect("close operator");

    assert_eq!(first.output, "FIRST TASK");
    assert_eq!(second.output, "SECOND TASK");
    assert_eq!(operator.agent.tasks, ["first task", "second task"]);
    assert!(operator.agent.closed);
}
