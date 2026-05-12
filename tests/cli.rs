use clap::Parser;
use millracer::cli::{Cli, Commands, Intake, OutputMode, PiSession, Route, run_from};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn binary_help_and_version_work() {
    let bin = env!("CARGO_BIN_EXE_millracer");

    let help = Command::new(bin).arg("--help").output().expect("run help");
    assert!(help.status.success());
    let help_text = String::from_utf8(help.stdout).expect("utf8 help");
    assert!(help_text.contains("run"));
    assert!(help_text.contains("operator"));

    let version = Command::new(bin)
        .arg("--version")
        .output()
        .expect("run version");
    assert!(version.status.success());
    let version_text = String::from_utf8(version.stdout).expect("utf8 version");
    assert!(version_text.contains("millracer 0.1.2"));
}

#[test]
fn run_parser_accepts_python_option_surface() {
    let cli = Cli::try_parse_from([
        "millracer",
        "run",
        "do",
        "work",
        "--route",
        "millrace",
        "--intake",
        "probe",
        "--workspace",
        "/tmp/ws",
        "--cwd",
        "/tmp/ws/project",
        "--millrace-mode",
        "custom_mode",
        "--thinking",
        "medium",
        "--provider",
        "openai",
        "--model",
        "gpt-test",
        "--skill",
        "/tmp/SKILL.md",
        "--pi-session",
        "print",
        "--output",
        "json",
        "--no-notify-terminal-stages",
    ])
    .expect("parse run args");

    let Some(Commands::Run(args)) = cli.command else {
        panic!("expected run command");
    };
    assert_eq!(args.task, ["do", "work"]);
    assert_eq!(args.common.route, Route::Millrace);
    assert_eq!(args.common.intake, Intake::Probe);
    assert_eq!(args.common.workspace.to_string_lossy(), "/tmp/ws");
    assert_eq!(
        args.common.cwd.as_ref().map(|path| path.to_string_lossy()),
        Some("/tmp/ws/project".into())
    );
    assert_eq!(args.common.millrace_mode, "custom_mode");
    assert_eq!(args.common.thinking, "medium");
    assert_eq!(args.common.provider.as_deref(), Some("openai"));
    assert_eq!(args.common.model.as_deref(), Some("gpt-test"));
    assert_eq!(args.common.skill.len(), 1);
    assert_eq!(args.pi_session, PiSession::Print);
    assert_eq!(args.output, OutputMode::Json);
    assert!(!args.common.notify_terminal_stages());
}

#[test]
fn operator_parser_accepts_common_options() {
    let cli = Cli::try_parse_from(["millracer", "operator", "--intake", "idea"])
        .expect("parse operator args");

    let Some(Commands::Operator(args)) = cli.command else {
        panic!("expected operator command");
    };
    assert_eq!(args.common.intake, Intake::Idea);
    assert!(args.common.notify_terminal_stages());
}

#[test]
fn run_uses_stdin_task_fallback() {
    let pi = fake_pi_command("stdin");
    let outcome = run_from(
        vec![
            "millracer".to_owned(),
            "run".to_owned(),
            "--route".to_owned(),
            "direct".to_owned(),
            "--pi-session".to_owned(),
            "print".to_owned(),
            "--pi-command".to_owned(),
            pi.display().to_string(),
            "--no-default-skills".to_owned(),
        ],
        "Fix the tests",
    );

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert!(outcome.stdout.contains("Fix the tests"));
}

#[test]
fn benchmark_json_output_accepts_external_request_without_commands() {
    let pi = fake_pi_command("benchmark");
    let workspace =
        std::env::temp_dir().join(format!("millracer-cli-benchmark-ws-{}", std::process::id()));
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let outcome = run_from(
        vec![
            "millracer".to_owned(),
            "run".to_owned(),
            "--benchmark-json".to_owned(),
            "--output".to_owned(),
            "json".to_owned(),
            "--route".to_owned(),
            "direct".to_owned(),
            "--pi-session".to_owned(),
            "print".to_owned(),
            "--pi-command".to_owned(),
            pi.display().to_string(),
            "--no-default-skills".to_owned(),
        ],
        &format!(
            r#"
        {{
          "prompt": "Fix the project and run the tests",
          "workspace": "{}",
          "intake_kind": "probe",
          "work_item": {{
            "id": "ITEM-123",
            "constraints": ["Only this item."]
          }}
        }}
        "#,
            workspace.display()
        ),
    );

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    let payload: Value = serde_json::from_str(&outcome.stdout).expect("json stdout");
    assert_eq!(payload["task"], "Fix the project and run the tests");
    assert_eq!(payload["workspace"], workspace.display().to_string());
    assert_eq!(payload["cwd"], workspace.display().to_string());
    assert_eq!(payload["intake_kind"], "probe");
    assert_eq!(payload["pi_session"], "print");
    assert_eq!(payload["notify_terminal_stages"], true);
    assert_eq!(payload["outcome"], "completed");
    assert_eq!(payload["scoped_completion"], false);
    assert_eq!(
        payload["completion_evidence"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(payload["scoped_work_item"]["item_id"], "ITEM-123");
}

fn fake_pi_command(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "millracer-cli-fake-pi-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("fake pi dir");
    let path = dir.join("pi");
    std::fs::write(
        &path,
        r#"#!/bin/sh
last=""
for arg in "$@"; do
  last="$arg"
done
case "$last" in
  *"Return only JSON"*) printf '{"decision":"direct","why":"test decision"}\n' ;;
  *"Millrace emitted this terminal event"*) printf 'final answer\n' ;;
  *) printf 'pi output: %s\n' "$last" ;;
esac
"#,
    )
    .expect("write fake pi");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod fake pi");
    }
    path
}
