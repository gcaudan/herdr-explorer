//! Terminal editor on a file, in a full-tab pane opened from the explorer or the preview: the editor of
//! the settings when it needs a terminal, $VISUAL, $EDITOR, else the first of nano, nvim, vim, vi (edit, nvim, vim,
//! nano on Windows). The pane closes when the editor exits. Source control's pane runs the tool of the settings
//! (scmCommand) the same way (tool).

use crate::core::exec::{self, WIN};
use crate::core::{paths, which};
use std::io::Write;
use std::time::Duration;

/// Absolute path of a program, or None when missing: found in the absolute folders of PATH only, as every program the
/// plugin runs (an empty or '.' entry of PATH would find one planted in the folder of the pane). A program given with
/// its folder must exist, else the next editor is tried.
fn find(name: &str) -> Option<String> {
    if !name.contains(['/', '\\']) {
        return which::which(name);
    }
    if !paths::is_absolute(name) {
        return None;
    }
    let names = if WIN && paths::extname(name).is_empty() {
        vec![name.to_string(), format!("{name}.exe")]
    } else {
        vec![name.to_string()]
    };
    names.into_iter().find(|full| {
        std::fs::metadata(full).is_ok_and(|m| m.is_file()) && {
            #[cfg(unix)]
            let ok = crate::sys::executable(full);
            #[cfg(not(unix))]
            let ok = true;
            ok
        }
    })
}

fn editor() -> Option<(String, Vec<String>)> {
    for v in ["EXPLORER_EDITOR", "VISUAL", "EDITOR"] {
        let words = exec::words(&exec::env_var(v).unwrap_or_default());
        if let Some((cmd, args)) = words.split_first()
            && !cmd.is_empty()
            && let Some(found) = find(cmd)
        {
            return Some((found, args.to_vec()));
        }
    }
    let names: [&str; 4] = if WIN { ["edit", "nvim", "vim", "nano"] } else { ["nano", "nvim", "vim", "vi"] };
    names.iter().find_map(|n| find(n)).map(|f| (f, Vec::new()))
}

/// A message, then the pane closed 3 s later, so that it can be read.
fn fail(text: &str) -> i32 {
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
    std::thread::sleep(Duration::from_millis(3000));
    1
}

pub fn run(_args: &[String]) -> i32 {
    let file = exec::env_var("EXPLORER_FILE");
    // The pane's folder, which new spaces, tabs and panes made from it start in (launch.rs); the file's without a
    // root.
    let dir = exec::env_var("EXPLORER_ROOT").or_else(|| file.as_deref().map(paths::dirname));
    crate::core::exec::home(dir.as_deref());
    let Some(file) = file else { return fail("No file.\r\n") };
    // A tab named "(Editor 2)", focused, handing over to the tab it was opened from at the exit, as source control's.
    if let Some(origin) = exec::env_var("EXPLORER_ORIGIN").filter(|o| !o.is_empty()) {
        crate::core::actions::hold_tab("edit", &origin, "Editor");
    }
    let Some((cmd, args)) = editor() else { return fail("No terminal editor found: set $EDITOR.\r\n") };
    launch(&cmd, &args, Some(&file), None)
}

/// The source control tool of the settings (scmCommand, in EXPLORER_TOOL), in the review's tab of source control's
/// pane, at the explorer's root (EXPLORER_ROOT): split as the editor's command, run without a shell.
pub fn tool() -> i32 {
    let root = exec::env_var("EXPLORER_ROOT").filter(|r| !r.is_empty());
    let review = exec::env_var("EXPLORER_REVIEW").unwrap_or_default();
    crate::core::actions::hold_tab("review", &review, "SC");
    let words = exec::words(&exec::env_var("EXPLORER_TOOL").unwrap_or_default());
    let Some((name, args)) = words.split_first().filter(|(n, _)| !n.is_empty()) else {
        return fail("No source control tool.\r\n");
    };
    let Some(cmd) = find(name) else { return fail(&format!("{name} not found.\r\n")) };
    launch(&cmd, args, None, root.as_deref())
}

/// cmd run with args (then the file), in dir when given; its exit code.
fn launch(cmd: &str, args: &[String], file: Option<&str>, dir: Option<&str>) -> i32 {
    // A batch file runs only through cmd, which would read its arguments: refused.
    let ext = paths::extname(cmd).to_lowercase();
    if WIN && (ext == ".cmd" || ext == ".bat") {
        return fail(&format!("Cannot start {cmd}: a batch file needs cmd\r\n"));
    }
    let mut command = std::process::Command::new(cmd);
    command.args(args).args(file);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    match command.status() {
        Err(e) => fail(&format!("Cannot start {cmd}: {}\r\n", exec::io_code(&e))),
        Ok(status) => match status.code() {
            // An exit code past a byte (Windows: 0xC000013A after ctrl+c), given as is.
            Some(code) if !(0..=255).contains(&code) => {
                exec::exit_hooks();
                std::process::exit(code)
            }
            // Killed by a signal: 0.
            code => code.unwrap_or(0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_with_a_relative_folder_is_not_found() {
        assert_eq!(find("./nano"), None);
        assert_eq!(find("bin/nano"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_program_with_its_folder_must_be_an_executable_file() {
        assert_eq!(find("/bin/sh").as_deref(), Some("/bin/sh"));
        assert_eq!(find("/etc/hostname-missing-file"), None);
        assert_eq!(find("/etc"), None);
    }
}
