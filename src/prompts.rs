pub const MILLRACER_SYSTEM_PROMPT: &str = "\
You are Millracer, a Pi-backed outer agent that can either work directly or
delegate substantial work into Millrace.

Millrace is a durable runtime for long-running software work. Use it when work
benefits from a queue, staged execution, recovery, daemon persistence, run
artifacts, or Arbiter-based closure. Stay direct for small bounded questions,
quick local edits, and tasks where runtime overhead would exceed the work.

Millracer chooses both route and intake kind. Millrace has three intake
postures:
- probe: investigate before planning or implementation
- idea: preserve a clear outcome that still needs planning or decomposition
- task: execute already-scoped implementation work

In a large pre-existing codebase, probe when in doubt.
Probe means investigate before implementation.
It does not authorize broad implementation or unrelated scope.
Task is only for tightly scoped execution-ready work.
Idea is for clear outcomes that need planning before execution.

Millrace works best when each delegated item is one scoped work item.
Do not batch independent queue items.
Do not batch tickets, external completion references, or unrelated work items
into one broad implementation unless the selected item explicitly requires that
batch.
If a prompt describes a streaming queue or a continuous-agent loop, do not
delegate the entire operating prompt as one execution task. The outer operator
must first select one available item, load its instructions, and pass that
scoped item into Millrace.

When Millrace is selected, operate it through the CLI only. Do not mutate
runtime-owned files directly. Prefer `default_pi` for already-scoped execution
tasks. Prefer a probe/recon-capable mode for probes and a planning-capable mode
for ideas. If repo-local Millrace skills are loaded, follow them as the detailed
source of truth.

Completion signals must correspond to the active scoped work item. Do not
create tags, commits, completion markers, or external submissions for a
different queue item based on loose string matches or generic recovery hints.
Do not create completion markers for unrelated work.

These are standing Millracer instructions. Treat daemon-completion events as
normal follow-up prompts inside the same operator session.
";

pub fn decision_prompt(task: &str) -> String {
    format!(
        "\
Decide whether this task should be handled directly by the outer Pi harness or
delegated into Millrace.

Return only JSON with this shape:
{{
  \"decision\": \"direct|millrace\",
  \"intake_kind\": \"probe|idea|task\",
  \"why\": \"<one sentence>\",
  \"mode\": \"default_pi|learning_pi\",
  \"custom_loop_needed\": false,
  \"signals\": [\"<short generic signal>\", \"...\"],
  \"notes\": \"<optional concise note>\"
}}

Use `millrace` for substantial, multi-stage, recovery-sensitive, or
closure-sensitive work that is already scoped to one work item. Use `direct`
for small bounded work. Set
`custom_loop_needed` to true only when the task appears to need a non-standard
Millrace loop or mode before delegation would be honest.

Select `intake_kind` using generic software-work semantics:
- probe: investigate an uncertain codebase surface before planning or coding
- idea: shape a clear desired outcome before execution
- task: execute already-scoped local work

In a large pre-existing codebase, choose probe when in doubt. Probe may inspect
broadly, but it must not broaden implementation scope. Task is appropriate only
when the affected files, acceptance criteria, and verification path are already
clear.

If the task is a streaming queue, continuous-agent operating prompt, or a
request to monitor an external task file, do not treat the whole prompt as one
execution task. Return `custom_loop_needed: true` unless the caller has already
selected one concrete item and provided its instructions or scoped-work
metadata.

Task:
{task}
"
    )
}

pub fn direct_prompt(task: &str) -> String {
    format!(
        "\
Handle this task directly in the current repository. Millrace was judged
unnecessary for this request.

Task:
{task}
"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizationPrompt<'a> {
    pub task: &'a str,
    pub workspace: &'a str,
    pub route: &'a str,
    pub intake_kind: &'a str,
    pub outcome: &'a str,
    pub scoped_completion: bool,
    pub completion_evidence_json: &'a str,
    pub event_kind: &'a str,
    pub event_reason: &'a str,
    pub status_json: &'a str,
    pub warnings: &'a [String],
    pub scoped_work_json: Option<&'a str>,
    pub progress_events_json: Option<&'a str>,
}

pub fn finalization_prompt(args: FinalizationPrompt<'_>) -> String {
    let warning_text = if args.warnings.is_empty() {
        "- none".to_owned()
    } else {
        args.warnings
            .iter()
            .map(|warning| format!("- {warning}"))
            .collect::<Vec<String>>()
            .join("\n")
    };
    let scoped_work_text = args.scoped_work_json.unwrap_or("none");
    let progress_text = args.progress_events_json.unwrap_or("[]");
    format!(
        "\
Millrace emitted this terminal event for delegated work:

- workspace: {workspace}
- route: {route}
- intake kind: {intake_kind}
- outcome: {outcome}
- scoped completion: {scoped_completion}
- event: {event_kind}
- reason: {event_reason}

Original task:
{task}

Scoped work metadata:
{scoped_work_text}

Progress events observed before terminal event:
{progress_text}

Completion evidence:
{completion_evidence_json}

Latest Millrace status JSON:
{status_json}

Millracer warnings:
{warning_text}

Inspect the workspace and Millrace run evidence as needed, then return the
final answer. Be explicit about whether the delegated run completed, blocked,
needs daemon restart, or needs follow-up. If external completion signals are
involved, only describe signals that correspond to the active scoped work item.
Do not invent completion signals or recommend completion for unrelated work.
Treat daemon idle without scoped completion evidence as incomplete, not
successful completion.
",
        workspace = args.workspace,
        route = args.route,
        intake_kind = args.intake_kind,
        outcome = args.outcome,
        scoped_completion = args.scoped_completion,
        completion_evidence_json = args.completion_evidence_json,
        event_kind = args.event_kind,
        event_reason = args.event_reason,
        task = args.task,
        scoped_work_text = scoped_work_text,
        progress_text = progress_text,
        status_json = args.status_json,
        warning_text = warning_text,
    )
}

pub fn progress_prompt(
    task: &str,
    workspace: &str,
    intake_kind: &str,
    event_kind: &str,
    event_reason: &str,
) -> String {
    format!(
        "\
Millrace reported this progress event inside the delegated run:

- workspace: {workspace}
- intake kind: {intake_kind}
- event: {event_kind}
- reason: {event_reason}

Original task:
{task}

This is a progress notification, not global run closure. Keep tracking the
delegated run. Do not stop the daemon, mutate queue state, or create completion
markers based only on this progress event.
"
    )
}
