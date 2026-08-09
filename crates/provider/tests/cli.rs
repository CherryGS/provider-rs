#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

#[test]
fn reports_supported_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_provider"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "usage: provider codex usage <auth path>"
    );
}
