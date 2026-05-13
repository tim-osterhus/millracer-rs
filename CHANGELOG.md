# Changelog

## 0.2.0 - Python v0.2.0 ops parity release readiness

- Bumped the Rust crate to the planned minor version `0.2.0` for parity with
  Python `millracer` `v0.2.0` at commit
  `7f52962b853c8720da304d33a3d3d28fd142b826`.
- Added the typed ops JSON boundary with schema `millracer.ops.v0.2`,
  `millracer ops --json`, structured request/result/event models, structured
  warnings and errors, and the reserved `--stream-json`
  `unsupported_transport` response.
- Added deterministic ops service dispatch for `status`, `enqueue`,
  `list_workspaces`, `select_workspace`, `list_sessions`, and
  `inspect_session`, including machine-readable `unsupported_action`,
  `workspace_unresolved`, and runtime failure results instead of arbitrary
  shell fallthrough.
- Added workspace resolution and lightweight session helpers while keeping
  session state limited to convenience data such as selected workspace/mode,
  recent request ids, and warning codes. Queue truth, work-item lifecycle
  truth, traces, artifacts, approvals, and terminal outcomes remain runtime
  authority, not session persistence.
- Preserved scoped-completion gating: external callers should treat delegated
  scoped work as complete only when ops completion or legacy JSON evidence
  reports positive scoped completion.
- Preserved legacy `run --benchmark-json --output json` compatibility by
  converting legacy requests into ops enqueue requests and rendering ops
  results back into the older one-shot JSON shape.
- Kept the package include allowlist limited to public crate files, Rust tests,
  and ops JSON fixtures while excluding `millrace-agents/`, `target/`, runtime
  state, and local operational artifacts from the public crate package.

Parity coverage for the Python `v0.2.0` reference tests:

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

Release verification targets for Arbiter:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all`
- `cargo metadata --no-deps --format-version 1`
- `cargo package --list`
- `cargo publish --dry-run`
- `git diff --check`

Normal Builder and Checker stages must record any dry-run failure as release
readiness evidence instead of publishing, tagging, pushing, uploading, or
deploying release artifacts.

## 0.1.2 - Python v0.1.5 patch parity release readiness

- Bumped the Rust crate to the planned patch version `0.1.2` for parity with
  Python `millracer` `v0.1.5` at commit
  `6b6f03996c9f644228ca996dc6cf57be891956f2`.
- Documented the Python `v0.1.5` result boundary fields: final `outcome`,
  scoped completion flag `scoped_completion`, and structured
  `completion_evidence` for delegated scoped-work completion.
- Preserved direct-route `outcome: "completed"` without scoped completion
  evidence, while delegated Arbiter or closed closure-target completion reports
  scoped completion with event evidence.
- Documented incomplete `idle_no_work` semantics: a drained daemon without
  scoped completion evidence reports `outcome: "incomplete"` and must not be
  treated as selected scoped-work completion.
- Covered the Python `v0.1.5` scoped-intake behavior, including newest-first
  reuse of existing scoped intake documents and blocked existing intake
  short-circuiting without a duplicate enqueue or false completion claim.
- Kept the package include allowlist limited to public crate files and
  continued to exclude ignored `millrace-agents/` operational state from the
  public crate package.

Parity coverage for the Python `v0.1.5` reference tests:

| Python reference | Rust coverage |
| --- | --- |
| `tests/test_agent.py` | `tests/agent.rs` |
| `tests/test_benchmark.py` | `tests/benchmark.rs` |
| `tests/test_cli.py` | `tests/cli.rs` |
| `tests/test_decision.py` | `tests/decision.rs` |
| `tests/test_intake.py` | `tests/intake.rs` |
| `tests/test_millrace.py` | `tests/millrace.rs`, `tests/scope.rs` |
| `tests/test_monitor.py` | `tests/monitor.rs` |
| `tests/test_operator.py` | `tests/operator.rs` |
| `tests/test_pi.py` | `tests/pi.rs` |
| `tests/test_prompts.py` | `tests/prompts.rs` |

Release verification targets for Arbiter:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all`
- `cargo package --list`
- `cargo publish --dry-run`

Normal Builder and Checker stages must record any dry-run failure as release
readiness evidence instead of publishing, tagging, pushing, uploading, or
deploying release artifacts.

## 0.1.1 - Bootstrap parity release readiness

- Bumped the Rust crate to the planned bootstrap version `0.1.1` for parity
  with Python `millracer` `v0.1.4`.
- Replaced the seed command shell with a library-backed `millracer` binary and
  public modules for CLI dispatch, benchmark JSON, decision parsing, intake
  selection, scoped-work metadata, prompt rendering, Pi print/RPC harnesses,
  Millrace controller behavior, daemon monitoring, agent orchestration, and
  persistent operator reuse.
- Documented the runtime dependency on external `pi` and `millrace` commands
  while keeping the Rust test suite independent of live external services
  through fake executors and process handles.
- Documented the external JSON request boundary, scoped-work metadata contract,
  Python truthiness-compatible fallback behavior, delegated intake commands,
  terminal-stage notifications, and non-publishing release posture.
- Expanded source package include coverage to the public crate files:
  `Cargo.toml`, `Cargo.lock`, `LICENSE`, `README.md`, `CHANGELOG.md`,
  `src/**/*.rs`, and `tests/**/*.rs`; Cargo-generated package metadata is
  expected, and ignored Millrace operational state under `millrace-agents/`
  remains outside the package.

Parity coverage for the Python `v0.1.4` reference tests:

| Python reference | Rust coverage |
| --- | --- |
| `tests/test_agent.py` | `tests/agent.rs` |
| `tests/test_benchmark.py` | `tests/benchmark.rs` |
| `tests/test_cli.py` | `tests/cli.rs` |
| `tests/test_decision.py` | `tests/decision.rs` |
| `tests/test_intake.py` | `tests/intake.rs` |
| `tests/test_millrace.py` | `tests/millrace.rs`, `tests/scope.rs` |
| `tests/test_monitor.py` | `tests/monitor.rs` |
| `tests/test_operator.py` | `tests/operator.rs` |
| `tests/test_pi.py` | `tests/pi.rs` |
| `tests/test_prompts.py` | `tests/prompts.rs` |

Release verification targets for Arbiter:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all`
- `cargo publish --dry-run`

Normal Builder and Checker stages must record any dry-run failure as release
readiness evidence instead of publishing, tagging, pushing, or uploading
artifacts.
