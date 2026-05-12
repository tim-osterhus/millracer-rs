use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::pi::{PiConfig, normalize_path, push_optional_pair};
use crate::prompts::MILLRACER_SYSTEM_PROMPT;
use crate::{MillracerError, MillracerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcStreamEvent {
    Line(String),
    Eof,
}

pub trait RpcChild {
    fn write_stdin_line(&mut self, line: &str) -> MillracerResult<()>;
    fn read_stdout_event(&mut self, timeout: Duration) -> MillracerResult<Option<RpcStreamEvent>>;
    fn stderr_text(&self) -> String;
    fn poll(&mut self) -> MillracerResult<Option<i32>>;
    fn close_stdin(&mut self) -> MillracerResult<()>;
    fn terminate(&mut self) -> MillracerResult<()>;
    fn kill(&mut self) -> MillracerResult<()>;
    fn wait(&mut self, timeout: Duration) -> MillracerResult<Option<i32>>;
}

pub trait PiSessionLike {
    fn prompt(&mut self, prompt: &str, timeout: Option<Duration>) -> MillracerResult<String>;
    fn close(&mut self) -> MillracerResult<()>;
}

pub trait PiSessionFactory {
    fn create(
        &self,
        command: Vec<String>,
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> MillracerResult<Box<dyn PiSessionLike>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPiSessionFactory;

impl PiSessionFactory for DefaultPiSessionFactory {
    fn create(
        &self,
        command: Vec<String>,
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> MillracerResult<Box<dyn PiSessionLike>> {
        Ok(Box::new(PiRpcSession::spawn(command, cwd, env)?))
    }
}

pub struct PiRpcHarness {
    pub config: PiConfig,
    pub env: Option<BTreeMap<String, String>>,
    session_factory: Box<dyn PiSessionFactory>,
    session: Option<Box<dyn PiSessionLike>>,
    cwd: Option<PathBuf>,
}

impl Default for PiRpcHarness {
    fn default() -> Self {
        Self::new(PiConfig::default())
    }
}

impl PiRpcHarness {
    pub fn new(config: PiConfig) -> Self {
        Self::with_session_factory(config, Box::new(DefaultPiSessionFactory))
    }

    pub fn with_session_factory(
        config: PiConfig,
        session_factory: Box<dyn PiSessionFactory>,
    ) -> Self {
        Self {
            config,
            env: None,
            session_factory,
            session: None,
            cwd: None,
        }
    }

    pub fn complete(
        &mut self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String> {
        self.session_for(cwd)?.prompt(prompt, timeout)
    }

    pub fn close(&mut self) -> MillracerResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.close()?;
        }
        self.session = None;
        self.cwd = None;
        Ok(())
    }

    fn session_for(&mut self, cwd: &Path) -> MillracerResult<&mut Box<dyn PiSessionLike>> {
        let resolved_cwd = normalize_path(cwd);
        if self.session.is_some() {
            if self.cwd.as_ref() != Some(&resolved_cwd) {
                return Err(MillracerError::message(format!(
                    "persistent Pi session is bound to {}; cannot reuse it for {}",
                    self.cwd
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "<unknown>".to_owned()),
                    resolved_cwd.display()
                )));
            }
        } else {
            let mut env = std::env::vars().collect::<BTreeMap<String, String>>();
            if let Some(overrides) = &self.env {
                env.extend(overrides.clone());
            }
            let session = self.session_factory.create(
                build_rpc_command(&self.config),
                &resolved_cwd,
                &env,
            )?;
            self.cwd = Some(resolved_cwd);
            self.session = Some(session);
        }

        let Some(session) = self.session.as_mut() else {
            return Err(MillracerError::message("pi rpc session was not created"));
        };
        Ok(session)
    }
}

pub fn build_rpc_command(config: &PiConfig) -> Vec<String> {
    let mut args = vec![
        config.command.clone(),
        "--mode".to_owned(),
        "rpc".to_owned(),
        "--no-session".to_owned(),
        "--no-context-files".to_owned(),
        "--append-system-prompt".to_owned(),
        MILLRACER_SYSTEM_PROMPT.to_owned(),
    ];
    push_optional_pair(&mut args, "--provider", config.provider.as_deref());
    push_optional_pair(&mut args, "--model", config.model.as_deref());
    push_optional_pair(&mut args, "--thinking", config.thinking.as_deref());
    for skill_path in &config.skill_paths {
        args.push("--skill".to_owned());
        args.push(skill_path.display().to_string());
    }
    args.extend(config.extra_args.iter().cloned());
    args
}

pub struct PiRpcSession {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    next_id: usize,
    closed: bool,
    child: Box<dyn RpcChild>,
}

impl PiRpcSession {
    pub fn spawn(
        command: Vec<String>,
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> MillracerResult<Self> {
        let child = Box::new(StdRpcChild::spawn(&command, cwd, env)?);
        Ok(Self::from_child(command, cwd, env.clone(), child))
    }

    pub fn from_child(
        command: Vec<String>,
        cwd: &Path,
        env: BTreeMap<String, String>,
        child: Box<dyn RpcChild>,
    ) -> Self {
        Self {
            command,
            cwd: cwd.to_path_buf(),
            env,
            next_id: 0,
            closed: false,
            child,
        }
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }

    fn ensure_open(&mut self) -> MillracerResult<()> {
        if self.closed {
            return Err(MillracerError::message("pi rpc session is closed"));
        }
        if let Some(code) = self.child.poll()? {
            return Err(MillracerError::message(format!(
                "pi rpc process exited with code {code}"
            )));
        }
        Ok(())
    }

    fn send(&mut self, payload: Value) -> MillracerResult<()> {
        self.child
            .write_stdin_line(&format!("{}\n", serde_json::to_string(&payload)?))
    }

    fn wait_for_response(
        &mut self,
        response_id: &str,
        deadline: Instant,
    ) -> MillracerResult<Value> {
        loop {
            let record = self.read_record(deadline)?;
            let Some(record) = record else {
                self.abort();
                return Err(MillracerError::message(format!(
                    "timed out waiting for pi rpc response {response_id}"
                )));
            };
            if record.get("type").and_then(Value::as_str) == Some("response")
                && record.get("id").and_then(Value::as_str) == Some(response_id)
            {
                return Ok(record);
            }
        }
    }

    fn last_assistant_text(&mut self) -> MillracerResult<String> {
        let response_id = self.id("last-assistant");
        self.send(json!({"id": response_id, "type": "get_last_assistant_text"}))?;
        let response =
            self.wait_for_response(&response_id, Instant::now() + Duration::from_secs(5))?;
        if !response
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(MillracerError::message(
                "pi rpc get_last_assistant_text failed",
            ));
        }
        Ok(response
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    fn read_record(&mut self, deadline: Instant) -> MillracerResult<Option<Value>> {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };
        let Some(event) = self.child.read_stdout_event(remaining)? else {
            return Ok(None);
        };
        let line = match event {
            RpcStreamEvent::Line(line) => line,
            RpcStreamEvent::Eof => {
                let stderr = self.child.stderr_text();
                let detail = if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                };
                return Err(MillracerError::message(format!(
                    "pi rpc stream ended{detail}"
                )));
            }
        };
        let payload = serde_json::from_str::<Value>(&line)
            .map_err(|error| MillracerError::message(format!("invalid pi rpc JSON: {error}")))?;
        if !payload.is_object() {
            return Err(MillracerError::message(
                "pi rpc record must be a JSON object",
            ));
        }
        Ok(Some(payload))
    }

    fn abort(&mut self) {
        let response_id = self.id("abort");
        let _ = self.send(json!({"id": response_id, "type": "abort"}));
        let _ = self.close();
    }
}

impl PiSessionLike for PiRpcSession {
    fn prompt(&mut self, prompt: &str, timeout: Option<Duration>) -> MillracerResult<String> {
        self.ensure_open()?;
        let timeout = timeout.unwrap_or_else(|| Duration::from_secs(3600));
        let timeout = timeout.max(Duration::from_secs(1));
        let deadline = Instant::now() + timeout;
        let prompt_id = self.id("prompt");
        self.send(json!({"id": prompt_id, "type": "prompt", "message": prompt}))?;
        let response = self.wait_for_response(&prompt_id, deadline)?;
        if !response
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let error = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("pi rpc prompt rejected");
            return Err(MillracerError::message(error.to_owned()));
        }

        loop {
            let Some(record) = self.read_record(deadline)? else {
                self.abort();
                return Err(MillracerError::message("pi rpc prompt timed out"));
            };
            if record.get("type").and_then(Value::as_str) == Some("agent_end") {
                break;
            }
        }

        self.last_assistant_text()
    }

    fn close(&mut self) -> MillracerResult<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let _ = self.child.close_stdin();
        if self.child.wait(Duration::from_secs(1))?.is_some() {
            return Ok(());
        }
        if self.child.poll()?.is_none() {
            self.child.terminate()?;
            if self.child.wait(Duration::from_secs(1))?.is_some() {
                return Ok(());
            }
        }
        if self.child.poll()?.is_none() {
            self.child.kill()?;
            let _ = self.child.wait(Duration::from_secs(1));
        }
        Ok(())
    }
}

struct StdRpcChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<RpcStreamEvent>,
    stderr: Arc<Mutex<String>>,
}

impl StdRpcChild {
    fn spawn(
        command: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> MillracerResult<Self> {
        let Some((program, rest)) = command.split_first() else {
            return Err(MillracerError::message("empty command"));
        };
        let mut child = Command::new(program)
            .args(rest)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MillracerError::message("pi rpc stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| MillracerError::message("pi rpc stderr is unavailable"))?;
        let (stdout_sender, stdout_receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = line
                            .trim_end_matches('\n')
                            .trim_end_matches('\r')
                            .to_owned();
                        if stdout_sender.send(RpcStreamEvent::Line(line)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = stdout_sender.send(RpcStreamEvent::Eof);
        });
        let stderr_text = Arc::new(Mutex::new(String::new()));
        let stderr_target = Arc::clone(&stderr_text);
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buffer = String::new();
            let _ = reader.read_to_string(&mut buffer);
            if let Ok(mut target) = stderr_target.lock() {
                target.push_str(&buffer);
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout: stdout_receiver,
            stderr: stderr_text,
        })
    }
}

impl RpcChild for StdRpcChild {
    fn write_stdin_line(&mut self, line: &str) -> MillracerResult<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| MillracerError::message("pi rpc stdin is unavailable"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    fn read_stdout_event(&mut self, timeout: Duration) -> MillracerResult<Option<RpcStreamEvent>> {
        match self.stdout.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(Some(RpcStreamEvent::Eof)),
        }
    }

    fn stderr_text(&self) -> String {
        self.stderr
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default()
    }

    fn poll(&mut self) -> MillracerResult<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().unwrap_or(1)))
    }

    fn close_stdin(&mut self) -> MillracerResult<()> {
        self.stdin.take();
        Ok(())
    }

    fn terminate(&mut self) -> MillracerResult<()> {
        self.child.kill()?;
        Ok(())
    }

    fn kill(&mut self) -> MillracerResult<()> {
        self.child.kill()?;
        Ok(())
    }

    fn wait(&mut self, timeout: Duration) -> MillracerResult<Option<i32>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status.code().unwrap_or(1)));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
