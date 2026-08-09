set quiet
set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

clippy_lints := "-D warnings -D clippy::expect_used -D clippy::unwrap_used"
nextest_args := "--all-features --locked --no-fail-fast --no-tests pass --status-level none --final-status-level fail --failure-output final --success-output never --show-progress none"

rust-clippy-fix:
    cargo clippy --fix --workspace --all-targets --all-features --locked -- {{ clippy_lints }}

rust-fmt:
    cargo fmt --all

rust-fmt-check:
    cargo fmt --all -- --check

rust-lint:
    cargo clippy --workspace --all-targets --all-features --locked -- {{ clippy_lints }}

rust-test-code package *args:
    cargo nextest run --package {{ package }} {{ nextest_args }} {{ args }}

rust-test-doc package *args:
    cargo test --doc --quiet --package {{ package }} --all-features --locked {{ args }}

rust-test-all:
    cargo nextest run --workspace {{ nextest_args }}

rust-finalize: rust-clippy-fix rust-fmt rust-fmt-check rust-lint rust-test-all
