use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub args: Vec<String>,
    pub returncode: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait ProcessHandle {
    fn poll(&mut self) -> MillracerResult<Option<i32>>;
    fn terminate(&mut self) -> MillracerResult<()>;
    fn kill(&mut self) -> MillracerResult<()>;
    fn wait(&mut self, timeout: Option<Duration>) -> MillracerResult<i32>;
}

pub trait CommandExecutor {
    fn run(
        &self,
        args: &[String],
        cwd: &Path,
        timeout: Option<Duration>,
        env: Option<&BTreeMap<String, String>>,
    ) -> MillracerResult<CommandResult>;

    fn start(
        &self,
        args: &[String],
        cwd: &Path,
        env: Option<&BTreeMap<String, String>>,
    ) -> MillracerResult<Box<dyn ProcessHandle>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub result: CommandResult,
}

impl Display for CommandError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let command = self.result.args.join(" ");
        let detail = if self.result.stderr.trim().is_empty() {
            self.result.stdout.trim()
        } else {
            self.result.stderr.trim()
        };
        let detail = if detail.is_empty() {
            "no output"
        } else {
            detail
        };
        write!(
            f,
            "command failed ({}): {}\n{}",
            self.result.returncode, command, detail
        )
    }
}

impl Error for CommandError {}

#[derive(Debug, Default, Clone, Copy)]
pub struct SubprocessExecutor;

impl CommandExecutor for SubprocessExecutor {
    fn run(
        &self,
        args: &[String],
        cwd: &Path,
        timeout: Option<Duration>,
        env: Option<&BTreeMap<String, String>>,
    ) -> MillracerResult<CommandResult> {
        let Some((program, rest)) = args.split_first() else {
            return Ok(CommandResult {
                args: Vec::new(),
                returncode: 127,
                stdout: String::new(),
                stderr: "empty command".to_owned(),
            });
        };

        let mut command = Command::new(program);
        command.args(rest).current_dir(cwd);
        if let Some(env) = env {
            command.envs(env);
        }
        let completed = if let Some(timeout) = timeout {
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            if wait_for_exit(&mut child, timeout).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MillracerError::message(format!(
                    "command timed out after {:.3}s: {}",
                    timeout.as_secs_f64(),
                    args.join(" ")
                )));
            }
            child.wait_with_output()?
        } else {
            command.output()?
        };
        Ok(CommandResult {
            args: args.to_vec(),
            returncode: completed.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&completed.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&completed.stderr).into_owned(),
        })
    }

    fn start(
        &self,
        args: &[String],
        cwd: &Path,
        env: Option<&BTreeMap<String, String>>,
    ) -> MillracerResult<Box<dyn ProcessHandle>> {
        let Some((program, rest)) = args.split_first() else {
            return Err(crate::MillracerError::message("empty command"));
        };
        let mut command = Command::new(program);
        command
            .args(rest)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(env) = env {
            command.envs(env);
        }
        Ok(Box::new(ChildHandle {
            child: command.spawn()?,
        }))
    }
}

#[derive(Debug)]
struct ChildHandle {
    child: Child,
}

impl ProcessHandle for ChildHandle {
    fn poll(&mut self) -> MillracerResult<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().unwrap_or(1)))
    }

    fn terminate(&mut self) -> MillracerResult<()> {
        #[cfg(unix)]
        {
            let pid = self.child.id() as libc::pid_t;
            // Match Python Popen.terminate by sending SIGTERM before kill fallback.
            let result = unsafe { libc::kill(pid, libc::SIGTERM) };
            if result == 0 {
                return Ok(());
            }
            Err(std::io::Error::last_os_error().into())
        }

        #[cfg(not(unix))]
        {
            self.child.kill()?;
            Ok(())
        }
    }

    fn kill(&mut self) -> MillracerResult<()> {
        self.child.kill()?;
        Ok(())
    }

    fn wait(&mut self, _timeout: Option<Duration>) -> MillracerResult<i32> {
        if let Some(timeout) = _timeout {
            let status = wait_for_exit(&mut self.child, timeout)
                .map_err(|_| MillracerError::message("process wait timed out"))?;
            Ok(status.code().unwrap_or(1))
        } else {
            Ok(self.child.wait()?.code().unwrap_or(1))
        }
    }
}

pub fn require_success(result: CommandResult) -> Result<CommandResult, CommandError> {
    if result.returncode == 0 {
        Ok(result)
    } else {
        Err(CommandError { result })
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<ExitStatus, ()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| ())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(());
        }
        sleep(Duration::from_millis(10));
    }
}
