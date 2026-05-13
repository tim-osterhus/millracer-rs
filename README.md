# Millracer

Millracer is a Rust implementation of the Python `millracer` operator harness.
The current Rust `0.2.0` minor parity release targets Python `millracer`
`v0.2.0` at commit `7f52962b853c8720da304d33a3d3d28fd142b826`. The previous
Rust `0.1.2` patch release remains the accepted Python `v0.1.5` parity
baseline.

Millracer keeps a Pi-backed outer operator session and can either answer a task
directly or delegate substantial work into a Millrace workspace. The Rust crate
ships both the `millracer` binary and a library surface for the CLI, typed ops
request/result/event models, ops service dispatch, workspace and session
helpers, legacy JSON compatibility adapters, agent, operator, Pi harness,
Millrace controller, daemon monitor, prompts, decision parsing, intake
selection, and scoped-work metadata.

## Requirements

Runtime use requires:

- `pi` on `PATH`
- `millrace` on `PATH`
- model and API credentials configured for Pi
- a Millrace workspace when using delegated routes

The test suite does not require live `pi` or `millrace` services. Rust parity
tests exercise those command boundaries with fake executors and fake process
handles.

## Install

```bash
cargo install millracer
```

From a local checkout:

```bash
cargo run -- --help
```

## Commands

Use `operator` for a persistent line-oriented session:

```bash
millracer operator --workspace /path/to/workspace
```

Use `run` for one task:

```bash
millracer run --workspace /path/to/workspace "Fix the failing tests"
```

Use `ops --json` for one typed Millrace OS request:

```bash
millracer ops --json < request.json
```

When no task argument is provided, `run` reads task text from stdin. `operator`
also accepts newline-separated tasks on stdin in non-interactive contexts.

By default, Millracer uses a persistent Pi RPC session for each `run` and across
all tasks in `operator`. For compatibility checks, pass `--pi-session print` to
use a fresh `pi --print` process per Pi turn.

## Ops JSON Boundary

Software callers should use `ops --json` for the stable typed command
boundary. The request and result use schema version `millracer.ops.v0.2`:

```json
{
  "schema_version": "millracer.ops.v0.2",
  "request_id": "req-001",
  "workspace_ref": {
    "root_path": "/path/to/workspace",
    "mode": "learning_codex"
  },
  "source": {
    "kind": "mission_control",
    "surface": "command_panel"
  },
  "action": "status",
  "input": {}
}
```

Current structured actions are `status`, `enqueue`, `list_workspaces`,
`select_workspace`, `list_sessions`, and `inspect_session`. Unsupported
actions return machine-readable `unsupported_action` errors instead of falling
through to arbitrary shell execution. Workspace resolution failures return
machine-readable `workspace_unresolved` errors.

`OpsResult` includes structured `warnings`, `errors`, `result`, and
`completion` fields. Runtime completion is never inferred from text alone. For
scoped delegated work, external callers should gate completion on
`completion.scoped_completion: true`.

`--stream-json` is reserved for event-frame streaming and currently returns a
structured `unsupported_transport` result.

## Legacy JSON Boundary

External automation that still uses the earlier one-shot compatibility shape
can pass one legacy request object on stdin:

```bash
printf '{"task":"Fix the project and run the tests","workspace":"/path/to/workspace"}' \
  | millracer run --benchmark-json --output json
```

The request accepts `task`, `prompt`, or `instructions`; optional `workspace`;
optional `intake_kind`; optional `metadata`; and scoped-work aliases including
`scoped_work_item`, `work_item`, `scope`, `scopedWorkItem`, and `workItem`.
Usable root values take precedence, while null, empty, invalid, or missing-id
aliases fall through to later valid aliases and metadata. Invalid intake strings
normalize away so metadata fallback can still match Python truthiness behavior.
New product integrations should use `millracer ops --json`; the
`--benchmark-json` option remains for compatibility with existing callers.

JSON output includes the selected route, intake kind, intake signals, decision
metadata, warnings, Millrace event data, status payload, progress events,
workspace, cwd, task path, Pi session mode, Millrace mode, terminal-stage
notification setting, scoped-work metadata, final outcome, scoped completion
flag, completion evidence, and final output text.

Rust `0.2.0` preserves the Python `v0.1.5` scoped-completion rule:
direct-route results report `outcome: "completed"` without scoped completion
evidence. Delegated runs report `outcome: "completed"`,
`scoped_completion: true`, and structured `completion_evidence` only when
Arbiter completion or a closed closure-target event proves the selected scoped
work completed. A drained daemon with no such evidence is reported as
`idle_no_work` with `outcome: "incomplete"` and must not be treated as selected
scoped-work completion.

## Scoped Work

Adapters for dynamic queues should pass exactly one selected item:

```json
{
  "task": "Implement the selected queue item only.",
  "workspace": "/path/to/workspace",
  "intake_kind": "probe",
  "scoped_work_item": {
    "item_id": "ITEM-123",
    "title": "Fix the selected failure",
    "source_queue": "/path/to/TASK_QUEUE.md",
    "spec_path": "/path/to/specs/ITEM-123.md",
    "completion_ref": "submit-ITEM-123",
    "constraints": ["Do not implement or submit any other queue item."]
  }
}
```

Millracer preserves this metadata in final JSON output and renders the
scoped-work contract into the Millrace intake document so delegated agents do
not batch unrelated queue items or emit completion signals for another item.

## Workspace And Sessions

`OpsRequest.workspace_ref` can identify a workspace by `workspace_id` or
`root_path`. Resolution prefers request fields, then CLI workspace defaults,
then selected/default session context when available. Unknown workspace ids can
fall back to a request root path; otherwise unresolved inputs return a
structured `workspace_unresolved` result.

Millracer may persist operator session convenience state such as selected
workspace, selected mode, recent request ids, and warning codes. Session state
does not persist queue truth, work-item lifecycle truth, traces, approvals,
artifacts, or terminal outcomes. The Millrace runtime remains authoritative.

## Routing And Intake

`--route auto` asks Pi to choose between direct work and Millrace delegation.
`--route direct` and `--route millrace` force a path.

`--intake auto` uses Pi's decision when available, then deterministic fallback
signals. The supported delegated intake kinds are:

- `probe`: investigation-first intake for uncertain codebase work
- `idea`: planning/decomposition intake for clear outcomes that need shaping
- `task`: execution intake for already-scoped local work

Delegated work initializes and validates the workspace, writes an intake
document under `.millracer/intake/`, dispatches it through the matching
`millrace queue add-probe`, `millrace queue add-idea`, or
`millrace queue add-task` command, and starts a `millrace run daemon` process
with `--monitor none`.

## Common Options

- `--workspace <path>`: Millrace workspace root and default command cwd.
- `--cwd <path>`: command cwd when it differs from the workspace.
- `--route auto|direct|millrace`: choose or force the route.
- `--intake auto|probe|idea|task`: choose or force delegated intake kind.
- `--pi-command <name>`: Pi command, default `pi`.
- `--millrace-command <name>`: Millrace command, default `millrace`.
- `--provider <name>` / `--model <name>`: Pi provider and model forwarding.
- `--thinking <level>`: Pi thinking level, default `high`.
- `--skill <path>`: forward skill packages or `SKILL.md` files to Pi.
- `--no-default-skills`: disable standard Millrace skill discovery.
- `--millrace-mode <mode>`: delegated Millrace mode, default `default_pi`.
- `--pi-session rpc|print`: persistent RPC or print-mode Pi execution.
- `--daemon-timeout-seconds <n>`: delegated daemon wait timeout.
- `--max-daemon-restarts <n>`: restart attempts after stopped-daemon signals.
- `--keep-daemon`: leave the daemon running after terminal events.
- `--notify-terminal-stages` / `--no-notify-terminal-stages`: enable or
  disable progress prompts for meaningful terminal-stage updates.
- `--output json`: machine-readable one-shot output.

## Parity Verification

The Rust test suite maps to the Python `v0.2.0` reference tests as follows:

| Python reference | Rust coverage |
| --- | --- |
| `tests/test_agent.py` | `tests/agent.rs` |
| `tests/test_benchmark.py` | `tests/benchmark.rs` |
| `tests/test_cli.py` | `tests/cli.rs` |
| `tests/test_decision.py` | `tests/decision.rs` |
| `tests/test_intake.py` | `tests/intake.rs` |
| `tests/test_millrace.py` | `tests/millrace.rs`, `tests/scope.rs` |
| `tests/test_monitor.py` | `tests/monitor.rs` |
| `tests/test_ops_models.py` | `tests/ops_models.rs` |
| `tests/test_ops_service.py` | `tests/ops_service.rs` |
| `tests/test_operator.py` | `tests/operator.rs` |
| `tests/test_pi.py` | `tests/pi.rs` |
| `tests/test_prompts.py` | `tests/prompts.rs` |
| `tests/test_sessions.py` | `tests/sessions.rs` |
| `tests/test_workspaces.py` | `tests/workspaces.rs` |

The parity checks are designed to run without live external services. They
cover CLI parsing, stdin task fallback, ops JSON input/output, legacy JSON
input/output, Python truthiness fallbacks, scoped-work aliases, decision
parsing, intake selection, prompt rendering, Pi print/RPC command construction,
persistent RPC sessions, Millrace controller command construction, intake
document rendering, daemon lifecycle handling, daemon monitor classification,
direct/delegated/auto agent routing, persistent operator reuse, workspace
resolution, session persistence, ops service dispatch, final outcome reporting,
scoped completion flags, completion evidence, and incomplete `idle_no_work`
semantics.

## Package And Release Posture

The source package include allowlist contains the public crate files:
`Cargo.toml`, `Cargo.lock`, `LICENSE`, `README.md`, `CHANGELOG.md`, `src/**/*.rs`,
`tests/**/*.rs`, and `tests/fixtures/**/*.json`. Cargo also adds generated
package metadata such as `.cargo_vcs_info.json` and `Cargo.toml.orig` during
packaging. Operational Millrace state under `millrace-agents/`, build output
under `target/`, runtime snapshots, and local auto-port intake state are
excluded from the crate package.

Normal Millrace execution stages validate release readiness with
`cargo package --list` and `cargo publish --dry-run`, but they do not publish,
tag, push, upload release artifacts, or otherwise perform deployment. Any local
dry-run failure remains release-readiness evidence for Arbiter or deployer
remediation. Publishing belongs to the deterministic auto-port deployer after
Arbiter accepts the completed parity lineage.
