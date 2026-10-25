//! The hook and peek subcommands, run as the binary: in an environment of their own (herdr's variables removed, a fake
//! herdr first on PATH, HERDR_BIN_PATH pointed at it, dummy values). The fake logs each call, its arguments separated
//! by tabs, and answers with the line of answers.tsv given for its arguments joined by spaces (the n-th one at the n-th
//! call, the last one after), else `{"result":null}`; FAIL makes it fail. A fake curl, for the update check, logs its
//! arguments and gives release.json when there is one, else fails as offline: no test reaches the network.
#![cfg(unix)]

mod support;

use herdr_explorer::json::{self, Value};
use std::path::PathBuf;
use std::process::{Command, Output};

const SCRIPT: &str = r#"#!/bin/sh
line=
for a in "$@"; do line="$line${line:+	}$a"; done
printf '%s\n' "$line" >>"$FAKE_HERDR_DIR/calls.log"
n=$(grep -cxF "$line" "$FAKE_HERDR_DIR/calls.log")
out=$(awk -F '\t' -v k="$*" -v n="$n" '$1 == k { c++; o = $2; if (c == n) { f = 1; exit } } END { print o }' \
  "$FAKE_HERDR_DIR/answers.tsv" 2>/dev/null)
if [ "$out" = FAIL ]; then exit 1; fi
if [ -n "$out" ]; then printf '%s\n' "$out"; else printf '%s\n' '{"result":null}'; fi
"#;

const CURL: &str = r#"#!/bin/sh
printf '%s\n' "$*" >>"$FAKE_HERDR_DIR/curl.log"
[ -f "$FAKE_HERDR_DIR/release.json" ] || exit 7
cat "$FAKE_HERDR_DIR/release.json"
"#;

struct Fake {
    tmp: support::TempDir,
    dir: PathBuf,
    state: PathBuf,
    config: PathBuf,
    bin: PathBuf,
    env: Vec<(String, String)>,
}

impl Fake {
    fn new(name: &str) -> Fake {
        use std::os::unix::fs::PermissionsExt;
        let tmp = support::TempDir::new(name);
        let dir = tmp.path.join("herdr");
        let state = tmp.path.join("state");
        let config = tmp.path.join("config");
        let bin = tmp.path.join("bin");
        for d in [dir.clone(), state.clone(), bin.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        let herdr = bin.join("herdr");
        std::fs::write(&herdr, SCRIPT).unwrap();
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        let curl = bin.join("curl");
        std::fs::write(&curl, CURL).unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = vec![
            ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
            ("HOME".into(), tmp.path.join("home").display().to_string()),
            ("HERDR_BIN_PATH".into(), herdr.display().to_string()),
            ("HERDR_BIN".into(), herdr.display().to_string()),
            ("HERDR_PLUGIN_STATE_DIR".into(), state.display().to_string()),
            ("HERDR_PLUGIN_CONFIG_DIR".into(), config.display().to_string()),
            ("FAKE_HERDR_DIR".into(), dir.display().to_string()),
        ];
        Fake { tmp, dir, state, config, bin, env }
    }

    fn set(&mut self, k: &str, v: &str) {
        self.env.retain(|(name, _)| name != k);
        self.env.push((k.into(), v.into()));
    }

    fn unset(&mut self, k: &str) {
        self.env.retain(|(name, _)| name != k);
    }

    fn answer(&self, args: &str, result: &str) {
        self.raw(args, &format!("{{\"result\":{result}}}"));
    }

    fn raw(&self, args: &str, out: &str) {
        use std::io::Write;
        let mut file = std::fs::File::options().append(true).create(true).open(self.dir.join("answers.tsv")).unwrap();
        writeln!(file, "{args}\t{out}").unwrap();
    }

    fn fails(&self, args: &str) {
        self.raw(args, "FAIL");
    }

    fn run(&self, sub: &str, args: &[&str]) -> Output {
        self.run_in(&self.tmp.path, sub, args)
    }

    fn run_in(&self, cwd: &std::path::Path, sub: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_herdr-explorer"))
            .arg(sub)
            .args(args)
            .env_clear()
            .envs(self.env.iter().cloned())
            .current_dir(cwd)
            .output()
            .unwrap()
    }

    /// The calls, each a list of its arguments.
    fn calls_args(&self) -> Vec<Vec<String>> {
        let log = std::fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default();
        log.lines().map(|l| l.split('\t').map(String::from).collect()).collect()
    }

    fn calls(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default();
        log.lines().map(|l| l.replace('\t', " ")).collect()
    }

    /// A tab t1 of workspace w1: panes as (id, label, cwd).
    fn tab(&self, panes: &[(&str, Option<&str>)]) {
        let list: Vec<String> = panes
            .iter()
            .map(|(id, label)| {
                let label = label.map_or("null".into(), |l| format!("\"{l}\""));
                format!(r#"{{"pane_id":"{id}","label":{label},"tab_id":"w1:t1","workspace_id":"w1","cwd":"/work"}}"#)
            })
            .collect();
        self.answer("pane list --workspace w1", &format!(r#"{{"panes":[{}]}}"#, list.join(",")));
        for (id, p) in panes.iter().zip(&list) {
            self.answer(&format!("pane get {}", id.0), &format!(r#"{{"pane":{p}}}"#));
        }
        self.answer(
            "workspace list",
            r#"{"workspaces":[{"workspace_id":"w1","focused":true,"active_tab_id":"w1:t2"}]}"#,
        );
    }

    /// The layout of the tab: p1 alone, 100 columns, in an area of 200.
    fn layout(&self, pane: &str) {
        let layout = r#"{"layout":{"area":{"width":200},"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":100}},{"pane_id":"w1:p7","rect":{"x":100,"y":0,"width":100}}]}}"#;
        self.answer(&format!("pane layout --pane {pane}"), layout);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const CTX: &str = r#"{"workspace_id":"w1","tab_id":"w1:t1","workspace_cwd":"/space"}"#;
const OPENED: &str = r#"{"plugin_pane":{"pane":{"pane_id":"w1:p9"}}}"#;

#[test]
fn the_state_folder_is_saved_for_peek_and_herdr_missing_ends_quietly() {
    let mut f = Fake::new("hook-state");
    let out = f.run("hook", &["--closed"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr)); // herdr answers no workspaces
    let saved = std::fs::read_to_string(f.config.join("state-dir")).unwrap();
    assert_eq!(saved, f.state.display().to_string());
    f.unset("HERDR_BIN_PATH");
    f.set("PATH", &f.tmp.path.join("empty").display().to_string());
    std::fs::remove_dir_all(&f.config).unwrap();
    assert_eq!(f.run("hook", &["--closed"]).status.code(), Some(0));
    assert!(!f.config.exists());
}

#[test]
fn toggle_opens_the_explorer_on_the_left_then_hands_the_focus_back() {
    let mut f = Fake::new("hook-open");
    f.set("HERDR_PLUGIN_CONTEXT_JSON", CTX);
    f.tab(&[("w1:p1", None), ("w1:p7", None)]);
    f.layout("w1:p1");
    f.answer(
        "plugin pane open --plugin gcaudan.explorer --entrypoint tree --placement split --target-pane w1:p1 --direction right --env EXPLORER_ROOT=/space --no-focus",
        OPENED,
    );
    for c in [
        "pane swap --source-pane w1:p9 --target-pane w1:p1",
        "pane focus --direction right --pane w1:p9",
        "tab focus w1:t2",
    ] {
        f.answer(c, "{}");
    }
    let out = f.run("hook", &["--toggle"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        f.calls(),
        [
            "pane list --workspace w1",
            "pane layout --pane w1:p1",
            "workspace list",
            "plugin pane open --plugin gcaudan.explorer --entrypoint tree --placement split --target-pane w1:p1 --direction right --env EXPLORER_ROOT=/space --no-focus",
            // (32 + 2) / 100 of the pane, its left part: 0.5 - 0.34
            "pane resize --pane w1:p1 --direction left --amount 0.160000",
            "pane swap --source-pane w1:p9 --target-pane w1:p1",
            "pane focus --direction right --pane w1:p9",
            "tab focus w1:t2",
        ]
    );
}

#[test]
fn toggle_docked_on_the_right_splits_the_top_right_pane() {
    let mut f = Fake::new("hook-right");
    std::fs::write(f.state.join("settings.json"), r#"{"side":"right"}"#).unwrap();
    f.set("HERDR_PLUGIN_CONTEXT_JSON", CTX);
    f.tab(&[("w1:p1", None), ("w1:p7", None)]);
    f.layout("w1:p1");
    let open = "plugin pane open --plugin gcaudan.explorer --entrypoint tree --placement split --target-pane w1:p7 --direction right --env EXPLORER_ROOT=/space --no-focus";
    f.answer(open, OPENED);
    for c in ["pane focus --direction left --pane w1:p9", "tab focus w1:t2"] {
        f.answer(c, "{}");
    }
    let out = f.run("hook", &["--toggle"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let calls = f.calls();
    assert!(calls.contains(&open.to_string()), "{calls:?}");
    // 1 - (32 + 2) / 100 - 0.5
    assert!(calls.contains(&"pane resize --pane w1:p7 --direction right --amount 0.160000".to_string()), "{calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("pane swap")), "{calls:?}");
}

#[test]
fn toggle_closes_the_explorer_and_its_preview() {
    let mut f = Fake::new("hook-close");
    f.set("HERDR_PLUGIN_CONTEXT_JSON", CTX);
    f.tab(&[("w1:p2", Some("Explorer")), ("w1:p1", None), ("w1:p5", Some("Preview"))]);
    f.answer("pane close w1:p5", "{}");
    f.answer("pane close w1:p2", "{}");
    let out = f.run("hook", &["--toggle"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(f.calls(), ["pane list --workspace w1", "pane close w1:p5", "pane close w1:p2"]);
    // herdr failing stops the hook, as the JS threw.
    f.fails("pane close w1:p5");
    assert_eq!(f.run("hook", &["--toggle"]).status.code(), Some(1));
    // A tab of the plugin's own panes only, or no tab: nothing.
    std::fs::remove_file(f.dir.join("answers.tsv")).unwrap();
    f.tab(&[("w1:p5", Some("Preview"))]);
    std::fs::remove_file(f.dir.join("calls.log")).unwrap();
    assert_eq!(f.run("hook", &["--toggle"]).status.code(), Some(0));
    assert_eq!(f.calls(), ["pane list --workspace w1"]);
    f.set("HERDR_PLUGIN_CONTEXT_JSON", "{}");
    assert_eq!(f.run("hook", &["--toggle"]).status.code(), Some(0));
    assert_eq!(f.calls().len(), 1);
}

#[test]
fn a_new_release_is_looked_for_once_a_day_unless_the_check_is_off() {
    let mut f = Fake::new("hook-update");
    f.set("HERDR_PLUGIN_EVENT_JSON", r#"{"data":{"tab":{"tab_id":"w1:t1","workspace_id":"w1"}}}"#);
    std::fs::write(f.state.join("settings.json"), r#"{"autoOpen":false}"#).unwrap();
    let release = r#"{"tag_name":"v9.10.0","html_url":"https://github.com/gcaudan/herdr-explorer/releases/tag/v9.10.0","body":"* **diff:** stage hunks ([abc1234](https://g/c))"}"#;
    std::fs::write(f.dir.join("release.json"), release).unwrap();
    assert_eq!(f.run("hook", &[]).status.code(), Some(0));
    let curl = std::fs::read_to_string(f.dir.join("curl.log")).unwrap();
    assert!(curl.ends_with("https://api.github.com/repos/gcaudan/herdr-explorer/releases/latest\n"), "{curl}");
    let saved = json::parse(&std::fs::read_to_string(f.state.join("update.json")).unwrap()).unwrap();
    assert_eq!(saved.get("latest"), Some(&Value::from("9.10.0")));
    assert_eq!(saved.get("repo"), Some(&Value::from("gcaudan/herdr-explorer")));
    assert_eq!(saved.get("notes"), Some(&Value::from(vec!["diff: stage hunks"])));
    assert!(saved.get("checkedAt").and_then(Value::as_f64).is_some());
    // Within the day: not asked again, from the hook nor from --restore.
    assert_eq!(f.run("hook", &[]).status.code(), Some(0));
    f.answer("workspace list", r#"{"workspaces":[]}"#);
    assert_eq!(f.run("hook", &["--restore"]).status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(f.dir.join("curl.log")).unwrap(), curl);
    // A day later, offline: checked, the release found before kept, not asked again that day.
    std::fs::write(f.state.join("update.json"), r#"{"checkedAt":1,"latest":"9.10.0","skip":"9.10.0"}"#).unwrap();
    std::fs::remove_file(f.dir.join("release.json")).unwrap();
    assert_eq!(f.run("hook", &["--restore"]).status.code(), Some(0));
    let saved = json::parse(&std::fs::read_to_string(f.state.join("update.json")).unwrap()).unwrap();
    assert_eq!(saved.get("latest"), Some(&Value::from("9.10.0")));
    assert_eq!(saved.get("skip"), Some(&Value::from("9.10.0")));
    assert!(saved.get("checkedAt").and_then(Value::as_f64).is_some_and(|t| t > 1.0));
    // The check off: nothing asked.
    std::fs::write(f.state.join("update.json"), "{}").unwrap();
    std::fs::write(f.state.join("settings.json"), r#"{"autoOpen":false,"updateCheck":false}"#).unwrap();
    std::fs::remove_file(f.dir.join("curl.log")).unwrap();
    assert_eq!(f.run("hook", &[]).status.code(), Some(0));
    assert!(!f.dir.join("curl.log").exists());
}

#[test]
fn a_new_tab_gets_the_explorer_unless_auto_open_is_off() {
    let mut f = Fake::new("hook-created");
    f.set("HERDR_PLUGIN_CONTEXT_JSON", r#"{"workspace_id":"w1","workspace_cwd":"/space"}"#);
    f.set("HERDR_PLUGIN_EVENT_JSON", r#"{"data":{"tab":{"tab_id":"w1:t1","workspace_id":"w1"}}}"#);
    f.tab(&[("w1:p1", None)]);
    f.layout("w1:p1");
    let open = "plugin pane open --plugin gcaudan.explorer --entrypoint tree --placement split --target-pane w1:p1 --direction right --env EXPLORER_ROOT=/space --no-focus";
    f.answer(open, OPENED);
    for c in [
        "pane swap --source-pane w1:p9 --target-pane w1:p1",
        "pane focus --direction right --pane w1:p9",
        "tab focus w1:t2",
    ] {
        f.answer(c, "{}");
    }
    let out = f.run("hook", &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(f.calls().contains(&open.to_string()));
    std::fs::remove_file(f.dir.join("calls.log")).unwrap();
    std::fs::write(f.state.join("settings.json"), r#"{"autoOpen":false}"#).unwrap();
    assert_eq!(f.run("hook", &[]).status.code(), Some(0));
    assert!(f.calls().is_empty());
    // A bad event is what the JS threw on.
    f.set("HERDR_PLUGIN_EVENT_JSON", "{");
    assert_eq!(f.run("hook", &[]).status.code(), Some(1));
}

#[test]
fn closed_removes_the_state_of_the_tabs_gone() {
    let f = Fake::new("hook-closed");
    f.tab(&[("w1:p1", None)]);
    std::fs::create_dir_all(f.state.join("panes")).unwrap();
    for name in ["notes-w1_t1.json", "notes-w1_t9.json", "panes/w1_p1.json", "panes/w1_p9.json"] {
        std::fs::write(f.state.join(name), "{}").unwrap();
    }
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    for name in ["notes-w1_t1.json", "notes-w1_t9.json", "panes/w1_p1.json", "panes/w1_p9.json"] {
        std::fs::File::options().write(true).open(f.state.join(name)).unwrap().set_modified(old).unwrap();
    }
    let out = f.run("hook", &["--closed"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let left: Vec<bool> = ["notes-w1_t1.json", "notes-w1_t9.json", "panes/w1_p1.json", "panes/w1_p9.json"]
        .iter()
        .map(|n| f.state.join(n).exists())
        .collect();
    assert_eq!(left, [true, false, true, false]);
}

#[test]
fn agent_reports_what_it_did_with_the_turn() {
    let mut f = Fake::new("hook-agent");
    f.set("HERDR_PLUGIN_EVENT_JSON", r#"{"data":{"pane_id":"w1:p1","agent_status":"idle"}}"#);
    f.tab(&[("w1:p1", None)]);
    let out = f.run("hook", &["--agent"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "kept\n");
    // herdr failing on the pane: nothing, as the JS caught it.
    f.fails("pane get w1:p1");
    let out = f.run("hook", &["--agent"]);
    assert_eq!((out.status.code(), text(&out.stdout)), (Some(0), String::new()));
}

#[test]
fn focused_leads_a_tab_under_review_to_its_review() {
    let mut f = Fake::new("hook-focused");
    f.set("HERDR_PLUGIN_EVENT_JSON", r#"{"data":{"tab":{"tab_id":"w1:t1"}}}"#);
    std::fs::write(f.state.join("review-w1_t1.json"), r#"{"pane":"w1:p8"}"#).unwrap();
    f.answer("pane get w1:p8", r#"{"pane":{"pane_id":"w1:p8","tab_id":"w1:t5","label":"Source Control"}}"#);
    f.answer("plugin pane focus w1:p8", "{}");
    f.answer("tab focus w1:t5", "{}");
    let out = f.run("hook", &["--focused"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(f.calls(), ["pane get w1:p8", "plugin pane focus w1:p8", "tab focus w1:t5"]);
}

#[test]
fn search_opens_from_the_tab_explorer_at_its_root() {
    let mut f = Fake::new("hook-search");
    f.set("HERDR_PLUGIN_CONTEXT_JSON", CTX);
    f.set("HERDR_PANE_ID", "w1:p1");
    f.tab(&[("w1:p2", Some("Explorer")), ("w1:p1", None)]);
    std::fs::create_dir_all(f.state.join("panes")).unwrap();
    std::fs::write(f.state.join("panes/w1_p2.json"), r#"{"root":"/tree"}"#).unwrap();
    f.answer("tab list --workspace w1", r#"{"tabs":[{"tab_id":"w1:t1"}]}"#);
    let out = f.run("hook", &["--search"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let calls = f.calls();
    let open = calls.iter().find(|c| c.starts_with("plugin pane open")).expect("the search opened");
    assert!(open.contains("--entrypoint search"), "{open}");
    assert!(open.contains("EXPLORER_ROOT=/tree"), "{open}");
}

#[test]
fn restore_reopens_the_panes_herdr_left_as_shells() {
    let f = Fake::new("hook-restore");
    let home = f.tmp.path.join("space");
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("a.txt");
    std::fs::write(&file, "A").unwrap();
    f.tab(&[("w1:p2", Some("Explorer")), ("w1:p1", None), ("w1:p5", Some("Preview"))]);
    // The explorer runs no node: a shell; the preview still runs.
    f.answer("pane process-info --pane w1:p2", r#"{"process_info":{"foreground_processes":[{"argv":["/bin/bash"]}]}}"#);
    f.answer(
        "pane process-info --pane w1:p5",
        r#"{"process_info":{"foreground_processes":[{"argv":["/usr/bin/node"]}]}}"#,
    );
    std::fs::create_dir_all(f.state.join("panes")).unwrap();
    std::fs::write(f.state.join("panes/w1_p2.json"), format!(r#"{{"home":"{}","root":"/gone"}}"#, home.display()))
        .unwrap();
    f.layout("w1:p1");
    let open = format!(
        "plugin pane open --plugin gcaudan.explorer --entrypoint tree --placement split --target-pane w1:p1 --direction right --env EXPLORER_ROOT={} --no-focus",
        home.display()
    );
    f.answer(&open, OPENED);
    for c in [
        "pane close w1:p2",
        "pane swap --source-pane w1:p9 --target-pane w1:p1",
        "pane focus --direction right --pane w1:p9",
        "tab focus w1:t2",
    ] {
        f.answer(c, "{}");
    }
    let out = f.run("hook", &["--restore"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let calls = f.calls();
    assert!(calls.contains(&"pane close w1:p2".to_string()), "{calls:?}");
    assert!(!calls.contains(&"pane close w1:p5".to_string()), "{calls:?}");
    assert!(calls.contains(&open), "{calls:?}");
    assert_eq!(calls.last().map(String::as_str), Some("pane list --workspace w1"), "swept at the end");
}

#[test]
fn restore_reopens_the_preview_at_its_shown_tab() {
    let f = Fake::new("hook-restore-preview");
    let dir = f.tmp.path.join("space");
    std::fs::create_dir_all(&dir).unwrap();
    for n in ["a.txt", "b.txt"] {
        std::fs::write(dir.join(n), n).unwrap();
    }
    let (a, b) = (dir.join("a.txt").display().to_string(), dir.join("b.txt").display().to_string());
    f.tab(&[("w1:p1", None), ("w1:p5", Some("Preview"))]);
    // Once closed, the preview is no longer listed.
    f.tab(&[("w1:p1", None)]);
    f.answer("pane process-info --pane w1:p5", r#"{"process_info":{"foreground_processes":[]}}"#);
    f.answer("pane close w1:p5", "{}");
    f.answer("plugin pane open --plugin gcaudan.explorer --entrypoint preview", OPENED);
    std::fs::create_dir_all(f.state.join("panes")).unwrap();
    // The tab shown (2) is b; the gone one is passed over.
    let tabs = format!(
        r#"{{"tabs":{{"list":[{{"file":"{a}"}},{{"file":"/gone.txt"}},{{"file":"{b}","view":"diff","root":"/r","kept":1}}],"current":2}}}}"#
    );
    std::fs::write(f.state.join("panes/w1_p5.json"), tabs).unwrap();
    let out = f.run("hook", &["--restore"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let calls = f.calls_args();
    let open = calls.iter().find(|c| c.starts_with(&["plugin".into(), "pane".into(), "open".into()])).expect("opened");
    let env = |k: &str| open.iter().find_map(|a| a.strip_prefix(&format!("{k}="))).map(String::from);
    assert_eq!(env("EXPLORER_FILE"), Some(b.clone()));
    assert_eq!(env("EXPLORER_ROOT").as_deref(), Some("/r"));
    assert_eq!(env("EXPLORER_LINE").as_deref(), Some("0"), "showPreview's default");
    assert_eq!(env("EXPLORER_VIEW").as_deref(), Some("diff"));
    let saved = json::parse(&env("EXPLORER_TABS").unwrap()).unwrap();
    assert_eq!(saved.get("current").and_then(Value::as_f64), Some(1.0));
    let files: Vec<&str> =
        saved.get("list").and_then(Value::as_array).unwrap().iter().filter_map(|t| t.get("file")?.as_str()).collect();
    assert_eq!(files, [a.as_str(), b.as_str()]);
}

// --- peek

fn peek_tab(f: &Fake) {
    f.tab(&[("w1:p1", None), ("w1:p5", Some("Preview"))]);
}

fn request(f: &Fake) -> Value {
    json::parse(&std::fs::read_to_string(f.state.join("preview-w1_t1.json")).unwrap()).unwrap()
}

#[test]
fn peek_shows_the_file_at_its_places() {
    let mut f = Fake::new("peek-places");
    f.set("HERDR_PANE_ID", "w1:p1");
    peek_tab(&f);
    let dir = f.tmp.path.display().to_string();
    std::fs::write(f.tmp.path.join("a.txt"), "A\n").unwrap();
    let out = f.run("peek", &["a.txt:12-14,30,42:5-18"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "Preview: a.txt:12-14,30,42:5-18\n");
    let r = request(&f);
    assert_eq!(r.get("file").and_then(Value::as_str), Some(format!("{dir}/a.txt").as_str()));
    assert_eq!(r.get("root").and_then(Value::as_str), Some(dir.as_str()));
    assert_eq!(r.get("line").and_then(Value::as_f64), Some(12.0));
    assert_eq!(
        r.get("marks").map(json::stringify).as_deref(),
        Some(r#"[{"from":12,"to":14,"a":0,"b":0},{"from":30,"to":30,"a":0,"b":0},{"from":42,"to":42,"a":5,"b":18}]"#)
    );
    assert_eq!(r.get("view").and_then(Value::as_str), Some("file"));

    // A deleted file has changes still: --diff only.
    let out = f.run("peek", &["gone.txt"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), format!("peek: no such file: {dir}/gone.txt\n")));
    let out = f.run("peek", &["--diff", "gone.txt"]);
    assert_eq!((out.status.code(), text(&out.stdout)), (Some(0), "Preview (diff): gone.txt\n".into()));
    assert_eq!(request(&f).get("view").and_then(Value::as_str), Some("diff"));
    let out = f.run("peek", &["."]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), format!("peek: not a file: {dir}\n")));
}

#[test]
fn peek_says_what_is_wrong_with_its_use() {
    let mut f = Fake::new("peek-use");
    let usage = "peek: usage: peek.js [--diff] <file>[:<line>[-<line>][:<col>-<col>],…] | [--diff] --last | --url\n";
    let out = f.run("peek", &["a.txt"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(2), "peek: run it from a herdr pane\n".into()));
    f.set("HERDR_PANE_ID", "w1:p1");
    for args in [&[][..], &["a", "b"], &["--help"], &["-h"]] {
        let out = f.run("peek", args);
        assert_eq!((out.status.code(), text(&out.stderr)), (Some(2), usage.into()), "{args:?}");
    }
    let out = f.run("peek", &["a.txt:0"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(2), "peek: wrong place: 0\n".into()));
    let out = f.run("peek", &["--url"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "peek: not a local file: (no link)\n".into()));
    f.set("HERDR_PLUGIN_CLICKED_URL", "https://example.com/x");
    let out = f.run("peek", &["--url"]);
    assert_eq!(text(&out.stderr), "peek: not a local file: https://example.com/x\n");
    // No preview to reach (the pane unknown to herdr).
    std::fs::write(f.tmp.path.join("a.txt"), "A").unwrap();
    let out = f.run("peek", &["a.txt"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "peek: Cannot preview\n".into()));
    f.unset("HERDR_BIN_PATH");
    f.set("PATH", &f.tmp.path.join("empty").display().to_string());
    let out = f.run("peek", &["a.txt"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "peek: herdr not found in PATH\n".into()));
}

#[test]
fn peek_url_shows_the_clicked_file() {
    let mut f = Fake::new("peek-url");
    f.set("HERDR_PANE_ID", "w1:p1");
    peek_tab(&f);
    let file = f.tmp.path.join("b c.txt");
    std::fs::write(&file, "B").unwrap();
    f.set("HERDR_PLUGIN_CLICKED_URL", &format!("file://{}#L4-L6", f.tmp.path.join("b%20c.txt").display()));
    let out = f.run("peek", &["--url"]);
    assert_eq!(
        (out.status.code(), text(&out.stdout)),
        (Some(0), "Preview: b c.txt:4-6\n".into()),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn peek_from_the_action_takes_the_tab_terminal() {
    let mut f = Fake::new("peek-action");
    peek_tab(&f);
    std::fs::write(f.tmp.path.join("a.txt"), "A").unwrap();
    let root = f.tmp.path.display().to_string();
    // The focus on the preview: the terminal's folder, /work, resolves the file.
    f.set("HERDR_PLUGIN_CONTEXT_JSON", r#"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p5"}"#);
    let out = f.run("peek", &[&format!("{root}/a.txt")]);
    assert_eq!(
        (out.status.code(), text(&out.stdout)),
        (Some(0), format!("Preview: {root}/a.txt\n")),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(request(&f).get("root").and_then(Value::as_str), Some("/work"));
}

#[test]
fn peek_finds_the_state_folder_the_hook_saved() {
    let mut f = Fake::new("peek-state");
    f.set("HERDR_PANE_ID", "w1:p1");
    f.unset("HERDR_PLUGIN_STATE_DIR");
    peek_tab(&f);
    std::fs::write(f.tmp.path.join("a.txt"), "A").unwrap();
    // Neither saved nor at herdr's usual place.
    let out = f.run("peek", &["a.txt"]);
    let msg = "peek: cannot find the plugin's state folder: open a new tab once so that the explorer saves it\n";
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), msg.into()));
    // herdr's usual place.
    let usual = f.tmp.path.join("xdg/herdr/plugins/gcaudan.explorer");
    std::fs::create_dir_all(&usual).unwrap();
    f.set("XDG_STATE_HOME", &f.tmp.path.join("xdg").display().to_string());
    assert_eq!(f.run("peek", &["a.txt"]).status.code(), Some(0));
    assert!(usual.join("preview-w1_t1.json").exists());
    // The one saved in the config folder herdr gives.
    std::fs::create_dir_all(&f.config).unwrap();
    std::fs::write(f.config.join("state-dir"), format!("{}\n", f.state.display())).unwrap();
    f.raw("plugin config-dir gcaudan.explorer", &format!("{}\n", f.config.display()));
    let out = f.run("peek", &["a.txt"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(f.state.join("preview-w1_t1.json").exists());
}

#[test]
fn peek_last_shows_the_changed_file_written_last() {
    let mut f = Fake::new("peek-last");
    f.set("HERDR_PANE_ID", "w1:p1");
    peek_tab(&f);
    // In a folder of its own: the fake's log and the state folder change at each run.
    let repo = f.tmp.path.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let out = f.run_in(&repo, "peek", &["--last"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "peek: not in a git repository\n".into()));
    let git = |args: &[&str]| {
        let ok = Command::new("git").args(args).current_dir(&repo).env("HOME", &f.tmp.path).output().unwrap();
        assert!(ok.status.success(), "{}", text(&ok.stderr));
    };
    git(&["init", "-q"]);
    let out = f.run_in(&repo, "peek", &["--last"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "peek: no changed file\n".into()));
    for (name, age) in [("old.txt", 60), ("new.txt", 5), ("mid.txt", 30)] {
        let p = repo.join(name);
        std::fs::write(&p, name).unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
    }
    let out = f.run_in(&repo, "peek", &["--last"]);
    assert_eq!((out.status.code(), text(&out.stdout)), (Some(0), "Preview: new.txt\n".into()), "{}", text(&out.stderr));
}

#[test]
fn the_fake_herdr_is_the_one_called() {
    // Décision 11: the binary never reaches the real herdr from these tests.
    let f = Fake::new("hook-fake");
    let out = f.run("hook", &["--closed"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(f.calls(), ["workspace list"]);
    assert!(f.bin.join("herdr").exists());
}
