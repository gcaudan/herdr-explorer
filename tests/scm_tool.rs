//! The source control tool of the settings (scmCommand): source control's pane runs it in its place, split as the
//! editor's command, without a shell, at the explorer's root. No herdr is involved.

mod support;

use std::process::{Command, Stdio};

#[test]
fn the_tool_runs_with_its_arguments_at_the_root() {
    let tmp = support::TempDir::new("scm-tool");
    let root = tmp.path.join("my repo");
    std::fs::create_dir_all(&root).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_herdr-explorer"))
        .args(["pane", "scm"])
        .env_remove("HERDR_PANE_ID")
        .env("EXPLORER_ROOT", &root)
        .env("EXPLORER_TOOL", "git init --quiet --initial-branch=tool")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    let head = std::fs::read_to_string(root.join(".git/HEAD")).unwrap();
    assert_eq!(head.trim(), "ref: refs/heads/tool");
}
