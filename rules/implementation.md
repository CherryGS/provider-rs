# Implementation rules

## Commands

Use Just recipes rather than raw Cargo commands.

- `just rust-clippy-fix` — apply machine-applicable lint fixes across the workspace.
- `just rust-fmt` — format the workspace.
- `just rust-fmt-check` — verify formatting without writing.
- `just rust-lint` — lint the workspace, all targets.
- `just rust-test-code <package> [args]` — run one package's code tests.
- `just rust-test-doc <package> [args]` — run one package's doc tests.
- `just rust-test-all` — run every workspace test.
- `just rust-finalize` — full gate: fix, format, format check, lint, test all.

## Rust rules

- Add `#![allow(clippy::expect_used, clippy::unwrap_used)]` at the start of every
  integration-test crate root, so unannotated helpers there may fail fast.
- Keep `expect_used` and `unwrap_used` denied in production code. Allow a
  deliberate production panic only at its smallest scope, with a stated reason.

## User preferences

(none)
