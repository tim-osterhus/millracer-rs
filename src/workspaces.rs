use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ops_models::OpsRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecord {
    pub workspace_id: String,
    pub root_path: PathBuf,
    pub display_name: Option<String>,
    pub default_mode: Option<String>,
    pub tags: Vec<String>,
}

impl WorkspaceRecord {
    pub fn new(workspace_id: impl Into<String>, root_path: impl Into<PathBuf>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            root_path: root_path.into(),
            display_name: None,
            default_mode: None,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceRegistry {
    pub records: BTreeMap<String, WorkspaceRecord>,
    pub default_workspace_id: Option<String>,
}

impl WorkspaceRegistry {
    pub fn empty() -> Self {
        Default::default()
    }

    pub fn get(&self, workspace_id: Option<&str>) -> Option<&WorkspaceRecord> {
        self.records.get(workspace_id?)
    }

    pub fn default(&self) -> Option<&WorkspaceRecord> {
        self.get(self.default_workspace_id.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceResolution {
    pub root_path: Option<PathBuf>,
    pub workspace_id: Option<String>,
    pub strategy: String,
    pub validated: bool,
    pub mode: Option<String>,
    pub display_name: Option<String>,
    pub error_code: Option<String>,
}

pub fn resolve_workspace(
    request: &OpsRequest,
    registry: &WorkspaceRegistry,
    cli_workspace: Option<&Path>,
    active_workspace_id: Option<&str>,
    cwd: Option<&Path>,
) -> WorkspaceResolution {
    let workspace_ref = &request.workspace_ref;
    if let Some(workspace_id) = &workspace_ref.workspace_id {
        if let Some(record) = registry.get(Some(workspace_id)) {
            return from_record(record, "request_workspace_id", workspace_ref.mode.clone());
        }
        if let Some(root_path) = &workspace_ref.root_path {
            return from_path(
                Path::new(root_path),
                "request_root_path",
                Some(workspace_id.clone()),
                workspace_ref.mode.clone(),
                workspace_ref.display_name.clone(),
            );
        }
        return unresolved("request_workspace_id", "workspace_unresolved");
    }

    if let Some(root_path) = &workspace_ref.root_path {
        return from_path(
            Path::new(root_path),
            "request_root_path",
            None,
            workspace_ref.mode.clone(),
            workspace_ref.display_name.clone(),
        );
    }

    if let Some(cli_workspace) = cli_workspace {
        return from_path(
            cli_workspace,
            "cli_workspace",
            None,
            workspace_ref.mode.clone(),
            None,
        );
    }

    if let Some(record) = registry.get(active_workspace_id) {
        return from_record(record, "active_session", workspace_ref.mode.clone());
    }

    if let Some(record) = registry.default() {
        return from_record(record, "registry_default", workspace_ref.mode.clone());
    }

    if let Some(cwd) = cwd {
        let root_path = resolve_path(cwd);
        if root_path.exists() {
            return WorkspaceResolution {
                root_path: Some(root_path),
                workspace_id: None,
                strategy: "cwd".to_owned(),
                validated: true,
                mode: workspace_ref.mode.clone(),
                display_name: None,
                error_code: None,
            };
        }
    }

    unresolved("unresolved", "workspace_unresolved")
}

fn from_record(
    record: &WorkspaceRecord,
    strategy: &str,
    mode: Option<String>,
) -> WorkspaceResolution {
    let root_path = resolve_path(&record.root_path);
    WorkspaceResolution {
        root_path: Some(root_path.clone()),
        workspace_id: Some(record.workspace_id.clone()),
        strategy: strategy.to_owned(),
        validated: root_path.exists(),
        mode: mode.or_else(|| record.default_mode.clone()),
        display_name: record.display_name.clone(),
        error_code: None,
    }
}

fn from_path(
    path: &Path,
    strategy: &str,
    workspace_id: Option<String>,
    mode: Option<String>,
    display_name: Option<String>,
) -> WorkspaceResolution {
    let root_path = resolve_path(path);
    WorkspaceResolution {
        root_path: Some(root_path.clone()),
        workspace_id,
        strategy: strategy.to_owned(),
        validated: root_path.exists(),
        mode,
        display_name,
        error_code: None,
    }
}

fn unresolved(strategy: &str, error_code: &str) -> WorkspaceResolution {
    WorkspaceResolution {
        root_path: None,
        workspace_id: None,
        strategy: strategy.to_owned(),
        validated: false,
        mode: None,
        display_name: None,
        error_code: Some(error_code.to_owned()),
    }
}

fn resolve_path(path: &Path) -> PathBuf {
    let expanded = expand_user(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(expanded)
    };

    absolute.canonicalize().unwrap_or(absolute)
}

fn expand_user(path: &Path) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    if raw == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
