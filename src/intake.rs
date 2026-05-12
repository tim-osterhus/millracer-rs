use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::decision::Decision;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntakeKind {
    Auto,
    Probe,
    Idea,
    Task,
}

impl IntakeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Probe => "probe",
            Self::Idea => "idea",
            Self::Task => "task",
        }
    }
}

impl Display for IntakeKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeDecision {
    pub intake_kind: IntakeKind,
    pub confidence: String,
    pub signals: Vec<String>,
}

pub fn normalize_intake_kind(value: &str, allow_auto: bool) -> Option<IntakeKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" if allow_auto => Some(IntakeKind::Auto),
        "probe" => Some(IntakeKind::Probe),
        "idea" => Some(IntakeKind::Idea),
        "task" => Some(IntakeKind::Task),
        _ => None,
    }
}

pub fn choose_intake_kind(
    task: &str,
    requested: IntakeKind,
    decision: Option<&Decision>,
) -> IntakeDecision {
    if requested != IntakeKind::Auto {
        return IntakeDecision {
            intake_kind: requested,
            confidence: "high".to_owned(),
            signals: vec![format!("forced --intake {}", requested.as_str())],
        };
    }

    if let Some(decision_kind) = decision
        .and_then(|decision| decision.intake_kind.as_deref())
        .and_then(|value| normalize_intake_kind(value, false))
    {
        return IntakeDecision {
            intake_kind: decision_kind,
            confidence: "medium".to_owned(),
            signals: decision
                .map(|decision| decision.signals.clone())
                .unwrap_or_default(),
        };
    }

    let signals = signals_for_task(task);
    if signals
        .iter()
        .any(|signal| signal == "exact local file and test")
    {
        return IntakeDecision {
            intake_kind: IntakeKind::Task,
            confidence: "medium".to_owned(),
            signals,
        };
    }
    if has_probe_signal(&signals) {
        return IntakeDecision {
            intake_kind: IntakeKind::Probe,
            confidence: "medium".to_owned(),
            signals,
        };
    }
    if has_idea_signal(&signals) {
        return IntakeDecision {
            intake_kind: IntakeKind::Idea,
            confidence: "medium".to_owned(),
            signals,
        };
    }
    if signals.iter().any(|signal| signal == "exact local file") && has_local_fix_signal(task) {
        return IntakeDecision {
            intake_kind: IntakeKind::Task,
            confidence: "medium".to_owned(),
            signals,
        };
    }

    IntakeDecision {
        intake_kind: IntakeKind::Probe,
        confidence: "low".to_owned(),
        signals: vec!["uncertain delegated repo work".to_owned()],
    }
}

pub fn signals_for_task(task: &str) -> Vec<String> {
    let text = task.to_ascii_lowercase();
    let mut signals = Vec::new();
    let has_file = has_source_path(task);
    let has_test = has_test_signal(task);
    if has_file && has_test {
        push_unique(&mut signals, "exact local file and test");
    } else if has_file {
        push_unique(&mut signals, "exact local file");
    }

    if text.contains("large pre-existing codebase") || text.contains("pre-existing codebase") {
        push_unique(&mut signals, "large pre-existing codebase");
    }
    if text.contains("uncertain")
        || text.contains("affected files")
        || text.contains("affected surface")
    {
        push_unique(&mut signals, "uncertain affected files");
    }
    if text.contains("understand the codebase") || text.contains("before changing") {
        push_unique(&mut signals, "understand before changing");
    }
    if text.contains("migration") || text.contains("migrate") || text.contains("refactor") {
        push_unique(&mut signals, "migration or refactor");
    }
    if text.contains("compatibility")
        || text.contains("regression")
        || text.contains("neighboring module")
        || text.contains("existing convention")
        || text.contains("cross-module")
        || text.contains("repo-wide")
        || text.contains("repository-wide")
    {
        push_unique(&mut signals, "compatibility or regression risk");
    }
    if text.contains("build a new")
        || text.contains("add a new")
        || text.contains("capability")
        || text.contains("dashboard")
        || text.contains("user-facing")
        || text.contains("operator-visible")
        || text.contains("workflow")
        || text.contains("tool behavior")
    {
        push_unique(&mut signals, "clear outcome needing shaping");
    }
    signals
}

fn push_unique(signals: &mut Vec<String>, signal: &str) {
    if !signals.iter().any(|candidate| candidate == signal) {
        signals.push(signal.to_owned());
    }
}

fn has_probe_signal(signals: &[String]) -> bool {
    signals.iter().any(|signal| {
        matches!(
            signal.as_str(),
            "large pre-existing codebase"
                | "uncertain affected files"
                | "understand before changing"
                | "migration or refactor"
                | "compatibility or regression risk"
        )
    })
}

fn has_idea_signal(signals: &[String]) -> bool {
    signals
        .iter()
        .any(|signal| signal == "clear outcome needing shaping")
}

fn has_local_fix_signal(task: &str) -> bool {
    let text = task.to_ascii_lowercase();
    ["fix", "mechanical", "exact", "local", "only", "acceptance"]
        .iter()
        .any(|phrase| text.contains(phrase))
}

fn has_test_signal(task: &str) -> bool {
    let text = task.to_ascii_lowercase();
    text.contains("pytest")
        || text.contains("failing test")
        || text.contains("/tests/")
        || text.contains("tests/")
        || text.contains("/test/")
        || text.contains("test_")
        || text.contains("::test_")
}

fn has_source_path(task: &str) -> bool {
    task.split_whitespace().any(token_has_source_path)
}

fn token_has_source_path(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let base = token.split("::").next().unwrap_or(token);
    let base = base.trim_end_matches(['.', ':', ',', ';']);
    let lower = base.to_ascii_lowercase();
    const EXTENSIONS: &[&str] = &[
        ".py", ".pyi", ".js", ".jsx", ".ts", ".tsx", ".rs", ".go", ".java", ".c", ".cc", ".cpp",
        ".h", ".hpp", ".md", ".rst", ".toml", ".yaml", ".yml", ".json", ".ini", ".cfg", ".css",
        ".scss", ".html",
    ];
    EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension) && lower.len() > extension.len())
}
