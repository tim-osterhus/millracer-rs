use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::MillracerResult;
use crate::command::{CommandExecutor, SubprocessExecutor, require_success};
use crate::prompts::MILLRACER_SYSTEM_PROMPT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiConfig {
    pub command: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub skill_paths: Vec<PathBuf>,
    pub extra_args: Vec<String>,
}

impl Default for PiConfig {
    fn default() -> Self {
        Self {
            command: "pi".to_owned(),
            provider: None,
            model: None,
            thinking: Some("high".to_owned()),
            skill_paths: Vec::new(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PiHarness<E = SubprocessExecutor> {
    pub config: PiConfig,
    pub executor: E,
}

impl Default for PiHarness<SubprocessExecutor> {
    fn default() -> Self {
        Self {
            config: PiConfig::default(),
            executor: SubprocessExecutor,
        }
    }
}

impl PiHarness<SubprocessExecutor> {
    pub fn new(config: PiConfig) -> Self {
        Self {
            config,
            executor: SubprocessExecutor,
        }
    }
}

impl<E> PiHarness<E>
where
    E: CommandExecutor,
{
    pub fn complete(
        &self,
        prompt: &str,
        cwd: &Path,
        timeout: Option<Duration>,
    ) -> MillracerResult<String> {
        let result = require_success(self.executor.run(
            &self.build_command(prompt),
            cwd,
            timeout,
            None::<&BTreeMap<String, String>>,
        )?)
        .map_err(|error| crate::MillracerError::message(error.to_string()))?;
        Ok(result.stdout.trim_end_matches('\n').to_owned())
    }

    pub fn build_command(&self, prompt: &str) -> Vec<String> {
        build_print_command(&self.config, prompt)
    }
}

pub fn build_print_command(config: &PiConfig, prompt: &str) -> Vec<String> {
    let mut args = vec![
        config.command.clone(),
        "--print".to_owned(),
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
    args.push(prompt.to_owned());
    args
}

pub fn discover_default_skill_paths(start: Option<&Path>) -> Vec<PathBuf> {
    let start_path = start
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_exe().ok())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let start_path = normalize_path(&start_path);
    let skill_dirs = [
        "millrace-autonomous-delegation",
        "millrace-ops-agent-manual",
    ];
    let relative_roots = [
        Path::new("dev/source/millrace/docs/skills"),
        Path::new("source/millrace/docs/skills"),
    ];

    for root in std::iter::once(start_path.as_path()).chain(start_path.ancestors().skip(1)) {
        for relative_root in &relative_roots {
            let candidate = root.join(relative_root);
            let paths = skill_dirs
                .iter()
                .map(|skill_dir| candidate.join(skill_dir))
                .collect::<Vec<PathBuf>>();
            if paths.iter().all(|path| path.exists()) {
                return paths;
            }
        }
    }
    Vec::new()
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let expanded = expand_home(path);
    if let Ok(canonical) = expanded.canonicalize() {
        return canonical;
    }
    if expanded.is_absolute() {
        return expanded;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&expanded))
        .unwrap_or(expanded)
}

fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub(crate) fn push_optional_pair(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    args.push(flag.to_owned());
    args.push(value.to_owned());
}
