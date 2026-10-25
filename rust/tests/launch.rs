//! The launcher (src/launch.js): the child started again on code 75, with what it asked for (environment, folder),
//! the ending signals passed on, and its last code given back. One test: the launcher changes the folder of the process
//! and catches its signals. The child is a script; no herdr is involved.
#![cfg(unix)]

mod support;

use std::ffi::OsString;

const CHILD: &str = r#"#!/bin/sh
n=$(cat "$DIR/count" 2>/dev/null || echo 0); n=$((n + 1)); echo $n > "$DIR/count"
echo "$n ${X:-none} $(pwd) $*" >> "$DIR/log"
case $n in
1) printf '%s\n' '{"restart":{"X":"one"}}' >> "$EXPLORER_IPC_FILE"
   printf '{"cwd":"%s"}\n' "$DIR/sub" >> "$EXPLORER_IPC_FILE"
   exit 75;;
2) printf '%s\n' '{"restart":{"X":"two"}}' >> "$EXPLORER_IPC_FILE"; exit 75;;
3) trap 'echo term >> "$DIR/log"; exit 9' TERM
   echo ready > "$DIR/ready"
   while :; do sleep 0.05; done;;
esac
"#;

#[test]
fn the_launcher_starts_its_child_again_until_it_quits() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = support::TempDir::new("launch");
    let dir = tmp.path.display().to_string();
    std::fs::create_dir_all(tmp.path.join("sub")).unwrap();
    let child = tmp.path.join("child");
    std::fs::write(&child, CHILD).unwrap();
    std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
    let start = std::env::current_dir().unwrap().display().to_string();
    // SAFETY: the only test of this binary.
    unsafe { std::env::set_var("DIR", &dir) };

    let ready = tmp.path.join("ready");
    let kill = std::thread::spawn(move || {
        for _ in 0..500 {
            if ready.exists() {
                let pid = std::process::id().to_string();
                std::process::Command::new("kill").args(["-TERM", &pid]).status().unwrap();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    let code = herdr_explorer::launch::launcher(&child, &[OsString::from("a"), OsString::from("b c")]);
    kill.join().unwrap();

    assert_eq!(code, 9);
    let log = std::fs::read_to_string(tmp.path.join("log")).unwrap();
    let sub = format!("{dir}/sub");
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [format!("1 none {start} a b c"), format!("2 one {sub} a b c"), format!("3 two {sub} a b c"), "term".into()]
    );
    // The messages' folder is gone.
    let prefix = format!("herdr-explorer-{}-", std::process::id());
    let left = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().starts_with(&prefix));
    assert!(!left);
    std::env::set_current_dir(&start).unwrap();
}
