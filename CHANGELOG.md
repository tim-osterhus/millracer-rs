# Changelog

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
