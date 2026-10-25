//! core::actions: what the JS would crash on, and the programs the actions start (browser, file manager, clipboard,
//! editor), here fakes first on a PATH of their own, with herdr's variables removed and a fake herdr.

mod support;

use herdr_explorer::core::actions;

#[cfg(unix)]
#[test]
fn a_copy_never_lands_on_a_broken_link() {
    // Node's cpSync aborts the process there.
    let tmp = support::TempDir::new("dup-link");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::fs::write(tmp.path.join("a.txt"), "A").unwrap();
    std::os::unix::fs::symlink("nowhere", tmp.path.join("a copy.txt")).unwrap();
    let res = actions::duplicate(&format!("{dir}/a.txt"));
    assert_eq!(res, (None, Some("Cannot duplicate: ERR_FS_CP_EEXIST".into())));
    assert_eq!(std::fs::read_link(tmp.path.join("a copy.txt")).unwrap().to_str(), Some("nowhere"));
}

#[cfg(unix)]
#[test]
fn a_broken_link_or_a_missing_file_is_not_duplicated() {
    // statSync throws in the JS.
    let tmp = support::TempDir::new("dup-broken");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::os::unix::fs::symlink("nowhere", tmp.path.join("l")).unwrap();
    assert_eq!(actions::duplicate(&format!("{dir}/l")), (None, Some("Cannot duplicate: ENOENT".into())));
    assert_eq!(actions::duplicate(&format!("{dir}/none")), (None, Some("Cannot duplicate: ENOENT".into())));
}

#[test]
fn a_rename_through_a_file_says_why() {
    // lstat throws ENOTDIR in the JS.
    let tmp = support::TempDir::new("rename-through");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::fs::write(tmp.path.join("f"), "F").unwrap();
    let f = herdr_explorer::core::paths::join(&[&dir, "f"]);
    assert_eq!(actions::rename(&f, "f/g").0, None);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "F");
}

// Linux: xdg-open and wl-copy, where macOS has open and pbcopy.
#[cfg(target_os = "linux")]
#[test]
fn the_actions_start_their_programs_with_the_path_as_an_argument() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = support::TempDir::new("programs");
    let dir = tmp.path.to_string_lossy().into_owned();
    let bin = format!("{dir}/bin");
    let log = format!("{dir}/log");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(format!("{dir}/state")).unwrap();
    std::fs::create_dir_all(format!("{dir}/d")).unwrap();
    for (name, body) in [
        ("xdg-open", format!("printf '%s\\n' \"$@\" > '{log}/xdg-open'")),
        ("wl-copy", format!("cat > '{log}/wl-copy'")),
        ("wl-paste", r"printf 'a\r\nb\rc'".to_string()),
        ("myedit", format!("printf '%s\\n' \"$@\" > '{log}/myedit'")),
        ("herdr", format!("printf '%s\\n' \"$@\" >> '{log}/herdr'; exit 1")),
    ] {
        let file = format!("{bin}/{name}");
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::create_dir_all(&log).unwrap();
    let settings = |editor: &str| {
        std::fs::write(format!("{dir}/state/settings.json"), format!("{{\"editor\":\"{editor}\"}}")).unwrap();
    };
    // SAFETY: the only test of this binary that touches the environment; the others never read it.
    unsafe {
        for (k, _) in std::env::vars() {
            if k.starts_with("HERDR_") {
                std::env::remove_var(k);
            }
        }
        std::env::set_var("PATH", format!("{bin}:/usr/bin:/bin"));
        std::env::set_var("HERDR_BIN_PATH", format!("{bin}/herdr"));
        std::env::set_var("HERDR_BIN", format!("{bin}/herdr"));
        std::env::set_var("HERDR_PLUGIN_STATE_DIR", format!("{dir}/state"));
        std::env::set_var("HERDR_PANE_ID", "fake-pane");
        std::env::set_var("WAYLAND_DISPLAY", "fake");
    }
    // What a program was given, once it ran (it is not waited for).
    let given = |name: &str| {
        let file = format!("{log}/{name}");
        for _ in 0..300 {
            if let Ok(text) = std::fs::read_to_string(&file)
                && !text.is_empty()
            {
                std::fs::remove_file(&file).unwrap();
                return text;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("{name} did not run");
    };
    let pdf = format!("{dir}/a b.pdf");
    std::fs::write(&pdf, "").unwrap();
    std::fs::write(format!("{dir}/run.sh"), "").unwrap();
    std::fs::write(format!("{dir}/x.txt"), "").unwrap();

    assert_eq!(actions::system(&pdf), "Opening a b.pdf");
    assert_eq!(given("xdg-open"), format!("{pdf}\n"));
    assert_eq!(actions::open(&pdf, "p").as_deref(), Some("Opening a b.pdf"));
    assert_eq!(given("xdg-open"), format!("{pdf}\n"));
    assert_eq!(actions::system(&format!("{dir}/run.sh")), "Not opened: file type not allowed");
    let page = "https://github.com/gcaudan/herdr-explorer#readme";
    assert_eq!(actions::web(page, "the documentation"), "Opening the documentation");
    assert_eq!(given("xdg-open"), format!("{page}\n"));
    assert_eq!(actions::web("http://example.com/", "x"), "Not opened: not a web page");
    assert_eq!(actions::reveal(&pdf), None);
    assert_eq!(given("xdg-open"), format!("{dir}\n"));
    assert_eq!(actions::app(&format!("{dir}/d"), "p").as_deref(), Some("Opening d"));
    assert_eq!(given("xdg-open"), format!("{dir}/d\n"));

    assert_eq!(actions::copy("héllo 'x'", "Path copied"), "Path copied");
    assert_eq!(std::fs::read_to_string(format!("{log}/wl-copy")).unwrap(), "héllo 'x'");
    assert_eq!(actions::clipboard().as_deref(), Some("a\nb\nc"));

    // The editor of the settings, its arguments first; a terminal one goes to the editor pane (herdr refusing here),
    // after the pane is asked for its tab twice: for a tab of its own it would be in (origin_of), then for the origin.
    settings("myedit --wait");
    assert_eq!(actions::editor(&format!("{dir}/x.txt"), "p").as_deref(), Some("myedit: x.txt"));
    assert_eq!(given("myedit"), format!("--wait\n{dir}/x.txt\n"));
    settings("nowhere-editor");
    assert_eq!(actions::editor(&format!("{dir}/x.txt"), "p").as_deref(), Some("nowhere-editor not found"));
    settings("vim -u NONE");
    assert_eq!(actions::editor(&format!("{dir}/x.txt"), "p").as_deref(), Some("Cannot open the editor"));
    assert_eq!(given("herdr"), "pane\nget\np\n".repeat(3));
}

#[cfg(unix)]
#[test]
fn a_pipe_is_not_duplicated_but_passed_over_in_a_folder() {
    let tmp = support::TempDir::new("dup-fifo");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::fs::create_dir(tmp.path.join("d")).unwrap();
    std::fs::write(tmp.path.join("d/f"), "F").unwrap();
    let made = std::process::Command::new("mkfifo").args([format!("{dir}/q"), format!("{dir}/d/q")]).status();
    assert!(made.unwrap().success());
    assert_eq!(actions::duplicate(&format!("{dir}/q")), (None, Some("Cannot duplicate: ERR_FS_CP_FIFO_PIPE".into())));
    assert_eq!(actions::duplicate(&format!("{dir}/d")), (Some(format!("{dir}/d copy")), None));
    let copied: Vec<_> = std::fs::read_dir(tmp.path.join("d copy")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(copied, ["f"]);
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_looked_at_is_not_said_deleted() {
    use std::os::unix::fs::PermissionsExt;
    if herdr_explorer::sys::getuid() == 0 {
        return;
    }
    let tmp = support::TempDir::new("rm-locked");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::fs::create_dir(tmp.path.join("locked")).unwrap();
    std::fs::write(tmp.path.join("locked/x"), "").unwrap();
    std::fs::set_permissions(tmp.path.join("locked"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let res = actions::remove(&format!("{dir}/locked/x"));
    std::fs::set_permissions(tmp.path.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(res.as_deref(), Some("Cannot delete: EACCES"));
    assert!(tmp.path.join("locked/x").exists());
}
