//! The full tab, and the other actions on herdr's side: herdr's answers faked by a script first on a PATH of
//! its own (HERDR_BIN_PATH pointed at it, herdr's variables removed), for a tab explorer (p2) | terminal (p3) |
//! preview (p5). The script logs each call, its arguments separated by tabs, and answers with the file named by its
//! arguments joined by spaces, else the file of its first two (pane-list), else as a plugin pane opened.
#![cfg(unix)]

mod support;

use herdr_explorer::core::actions::{self, Kept, Line, Shown, WidthKeeper};
use herdr_explorer::core::exec;
use herdr_explorer::json::{self, Value};
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

struct Fake {
    _guard: std::sync::MutexGuard<'static, ()>,
    _tmp: support::TempDir,
    dir: PathBuf,
    state: PathBuf,
}

const SCRIPT: &str = r#"#!/bin/sh
line=
for a in "$@"; do line="$line${line:+	}$a"; done
printf '%s\n' "$line" >>"$FAKE_HERDR_DIR/calls.log"
if [ -f "$FAKE_HERDR_DIR/answers/$*" ]; then cat "$FAKE_HERDR_DIR/answers/$*"
elif [ -f "$FAKE_HERDR_DIR/$1-$2" ]; then cat "$FAKE_HERDR_DIR/$1-$2"
else printf '%s\n' '{"result":{"plugin_pane":{"pane":{"pane_id":"w1:p9"}}}}'; fi
"#;

fn pane(id: &str, label: Option<&str>, tab: &str) -> Value {
    json::parse(&format!(
        r#"{{"pane_id":"{id}","label":{},"tab_id":"{tab}","workspace_id":"w1"}}"#,
        label.map_or("null".into(), |l| format!("\"{l}\""))
    ))
    .unwrap()
}

fn answer(v: &Value) -> String {
    format!("{{\"result\":{}}}\n", json::stringify(v))
}

impl Fake {
    fn new() -> Fake {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = support::TempDir::new("full-tab");
        let dir = tmp.path.join("herdr");
        let state = tmp.path.join("state");
        std::fs::create_dir_all(dir.join("answers")).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let bin = tmp.path.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let herdr = bin.join("herdr");
        std::fs::write(&herdr, SCRIPT).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: the tests of this binary hold LOCK while they touch or read the environment.
        unsafe {
            for (k, _) in std::env::vars() {
                if k.starts_with("HERDR_") {
                    std::env::remove_var(k);
                }
            }
            std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
            // The fake of the first test: herdr's program is found once per process.
            std::env::set_var("HERDR_BIN_PATH", fake_bin(&herdr));
            std::env::set_var("HERDR_BIN", fake_bin(&herdr));
            std::env::set_var("HERDR_PLUGIN_STATE_DIR", &state);
            std::env::set_var("FAKE_HERDR_DIR", &dir);
        }
        Fake { _guard: guard, _tmp: tmp, dir, state }
    }

    /// The panes of the tab, and the neighbours the fake gives.
    fn reset(&self, panes: &[Value]) {
        let _ = std::fs::remove_dir_all(self.dir.join("answers"));
        let _ = std::fs::remove_file(self.dir.join("calls.log"));
        std::fs::create_dir_all(self.dir.join("answers")).unwrap();
        for p in panes {
            let id = p.get("pane_id").and_then(Value::as_str).unwrap();
            let one = Value::Obj(vec![("pane".into(), p.clone())]);
            self.answer(&format!("pane get {id}"), &one);
        }
        std::fs::write(
            self.dir.join("pane-list"),
            answer(&Value::Obj(vec![("panes".into(), Value::Arr(panes.to_vec()))])),
        )
        .unwrap();
        for (from, to) in [("w1:p2", "w1:p3"), ("w1:p3", "w1:p5")] {
            let n = json::parse(&format!(r#"{{"neighbor":{{"neighbor_pane_id":"{to}"}}}}"#)).unwrap();
            self.answer(&format!("pane neighbor --direction right --pane {from}"), &n);
        }
    }

    fn answer(&self, args: &str, v: &Value) {
        std::fs::write(self.dir.join("answers").join(args), answer(v)).unwrap();
    }

    fn calls(&self) -> Vec<Vec<String>> {
        let log = std::fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default();
        log.lines().map(|l| l.split('\t').map(String::from).collect()).collect()
    }

    fn opened(&self) -> Option<Vec<String>> {
        self.calls().into_iter().find(|a| a.len() > 2 && a[0] == "plugin" && a[2] == "open")
    }

    fn closed(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|a| a.len() > 2 && a[0] == "pane" && a[1] == "close")
            .map(|a| a[2].clone())
            .collect()
    }

    fn called(&self, line: &str) -> bool {
        self.calls().iter().any(|a| a.join(" ") == line)
    }
}

static FIRST: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// herdr's program is found once per process: every test points at the first test's fake, which reads the answers of
/// the test running from FAKE_HERDR_DIR. Copied out of the first test's folder, which goes with it, into cargo's
/// folder for the tests' files, which `cargo clean` empties.
fn fake_bin(herdr: &std::path::Path) -> String {
    FIRST
        .get_or_init(|| {
            let keep =
                std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("full-tab-{}", std::process::id()));
            std::fs::create_dir_all(&keep).unwrap();
            let bin = keep.join("herdr");
            std::fs::copy(herdr, &bin).unwrap();
            bin.display().to_string()
        })
        .clone()
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).map(|i| args[i + 1].clone())
}

fn tmp() -> String {
    std::env::temp_dir().display().to_string()
}

fn tmp_file(name: &str) -> String {
    herdr_explorer::core::paths::join(&[&tmp(), name])
}

// Zoomed in the agent's tab, herdr's zoom (one per tab, following the focus) went to the agent when the focus came
// back to it: the agent alone, the explorer hidden. Source control opens a review in a tab of its own instead.
#[test]
fn source_control_opens_a_review_of_the_tab_in_a_tab_of_its_own() {
    let f = Fake::new();
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", None, "w1:t1"),
        pane("w1:p5", Some("Preview"), "w1:t1"),
    ]);
    assert_eq!(actions::scm(&tmp(), "w1:p2", "", false), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--placement").as_deref(), Some("tab"));
    assert_eq!(option(&args, "--workspace").as_deref(), Some("w1"));
    assert!(args.iter().any(|a| a == "EXPLORER_REVIEW=w1:t1"));
    assert!(args.iter().any(|a| a == "--focus"));
    assert!(f.closed().is_empty());
}

#[test]
fn a_review_already_open_is_focused_not_opened_again() {
    let f = Fake::new();
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", None, "w1:t1"),
        pane("w1:p7", Some("Source Control"), "w1:t2"),
    ]);
    let review = f.state.join("review-w1_t1.json");
    std::fs::write(&review, r#"{"pane":"w1:p7"}"#).unwrap();
    assert_eq!(actions::scm(&tmp(), "w1:p2", "", false), None);
    assert_eq!(f.opened(), None);
    assert!(f.called("plugin pane focus w1:p7"));
    // Its pane gone (the review closed without its cleanup): a new one opens.
    f.reset(&[pane("w1:p2", Some("Explorer"), "w1:t1"), pane("w1:p3", None, "w1:t1")]);
    assert_eq!(actions::scm(&tmp(), "w1:p2", "", false), None);
    assert_eq!(option(&f.opened().unwrap(), "--placement").as_deref(), Some("tab"));
    std::fs::remove_file(review).unwrap();
}

#[test]
fn changes_open_on_the_right_of_the_terminal_in_place_of_the_preview() {
    let f = Fake::new();
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", None, "w1:t1"),
        pane("w1:p5", Some("Preview"), "w1:t1"),
    ]);
    assert_eq!(actions::diff(&tmp_file("a.txt"), &tmp(), "w1:p2"), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--placement").as_deref(), Some("split"));
    assert_eq!(option(&args, "--target-pane").as_deref(), Some("w1:p3"));
    assert_eq!(option(&args, "--direction").as_deref(), Some("right"));
    assert_eq!(f.closed(), ["w1:p5"]);
}

#[test]
fn the_editor_opens_in_a_tab_of_its_own_in_the_panes_workspace() {
    let f = Fake::new();
    f.reset(&[pane("w1:p2", Some("Explorer"), "w1:t1"), pane("w1:p3", None, "w1:t1")]);
    assert_eq!(actions::edit(&tmp_file("a.txt"), "w1:p2", Some("nano")), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--placement").as_deref(), Some("tab"));
    assert_eq!(option(&args, "--workspace").as_deref(), Some("w1"));
    assert!(!args.iter().any(|a| a == "--target-pane"));
}

#[test]
fn a_preview_asked_from_a_review_opens_beside_its_source_control_at_full_height() {
    let f = Fake::new();
    f.reset(&[pane("w1:p4", Some("Source Control"), "w1:t2")]);
    let o = Shown { view: "diff".into(), beside: Some("w1:p4".into()), ..Default::default() };
    assert_eq!(actions::preview(&tmp_file("a.txt"), &tmp(), "w1:p4", 0.0, false, true, &o), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--entrypoint").as_deref(), Some("preview"));
    assert_eq!(option(&args, "--target-pane").as_deref(), Some("w1:p4"));
    assert_eq!(option(&args, "--direction").as_deref(), Some("right"));
    assert!(f.closed().is_empty());
}

#[test]
fn a_commit_asked_from_a_review_opens_the_preview_on_it_beside_its_source_control() {
    let f = Fake::new();
    f.reset(&[pane("w1:p4", Some("Source Control"), "w1:t2")]);
    let show = json::parse(&format!(
        r#"{{"kind":"commit","ref":"a91ff98","paths":[],"title":"a91ff98 fix","cwd":{}}}"#,
        json::stringify(&Value::Str(tmp()))
    ))
    .unwrap();
    let o = Shown { show: Some(show.clone()), beside: Some("w1:p4".into()), ..Default::default() };
    assert_eq!(actions::preview("", &tmp(), "w1:p4", 0.0, false, true, &o), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--target-pane").as_deref(), Some("w1:p4"));
    let env = args.iter().find_map(|a| a.strip_prefix("EXPLORER_SHOW=")).unwrap();
    assert_eq!(json::parse(env).unwrap(), show);
    // In a review, q in the preview closes it, whatever it shows.
    assert!(args.iter().any(|a| a == "EXPLORER_REVIEW=1"));
    // A preview already open: the request file carries it.
    f.reset(&[pane("w1:p4", Some("Source Control"), "w1:t2"), pane("w1:p5", Some("Preview"), "w1:t2")]);
    assert_eq!(actions::preview("", &tmp(), "w1:p4", 0.0, false, false, &o), None);
    let req = exec::read_text(actions::request_file("w1:t2").unwrap()).unwrap();
    assert_eq!(json::parse(&req).unwrap().get("show"), Some(&show));
}

#[test]
fn the_search_opens_in_a_tab_of_its_own_or_focuses_the_one_already_open_for_the_tab() {
    let f = Fake::new();
    f.reset(&[pane("w1:p2", Some("Explorer"), "w1:t1"), pane("w1:p3", None, "w1:t1")]);
    assert_eq!(actions::search(&tmp(), "w1:p2", "content", "foo"), None);
    let args = f.opened().unwrap();
    assert_eq!(option(&args, "--placement").as_deref(), Some("tab"));
    assert_eq!(option(&args, "--workspace").as_deref(), Some("w1"));
    assert!(["EXPLORER_ORIGIN=w1:t1", "EXPLORER_PANE=w1:p2", "--focus"].iter().all(|x| args.iter().any(|a| a == x)));
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", None, "w1:t1"),
        pane("w1:p8", Some("Search"), "w1:t3"),
    ]);
    let search = f.state.join("search-w1_t1.json");
    std::fs::write(&search, r#"{"pane":"w1:p8"}"#).unwrap();
    assert_eq!(actions::search(&tmp(), "w1:p2", "files", ""), None);
    assert_eq!(f.opened(), None);
    assert!(f.called("plugin pane focus w1:p8"));
    // Asked from the search's own tab (its preview): that search, not another one.
    f.reset(&[pane("w1:p8", Some("Search"), "w1:t3"), pane("w1:p9", Some("Preview"), "w1:t3")]);
    assert_eq!(actions::search(&tmp(), "w1:p9", "files", ""), None);
    assert_eq!(f.opened(), None);
    std::fs::remove_file(search).unwrap();
}

#[test]
fn a_file_opened_from_the_search_is_written_for_the_tabs_explorer_to_select_even_when_the_preview_opens() {
    let f = Fake::new();
    f.reset(&[pane("w1:p2", Some("Explorer"), "w1:t1"), pane("w1:p3", None, "w1:t1")]);
    let file = tmp_file("a.txt");
    let o = Shown { reveal: true, ..Default::default() };
    assert_eq!(actions::preview(&file, &tmp(), "w1:p2", 3.0, false, true, &o), None);
    let req = json::parse(&exec::read_text(actions::request_file("w1:t1").unwrap()).unwrap()).unwrap();
    assert_eq!(req.get("file").and_then(Value::as_str), Some(file.as_str()));
    assert_eq!(req.get("reveal"), Some(&Value::Bool(true)));
}

// --- the other actions

#[test]
fn a_path_goes_to_the_terminal_quoted_for_its_shell_or_for_its_agent() {
    let f = Fake::new();
    f.reset(&[pane("w1:p2", Some("Explorer"), "w1:t1"), pane("w1:p3", None, "w1:t1")]);
    let mut term = pane("w1:p3", None, "w1:t1");
    term.set("cwd", Value::Str("/src".into()));
    f.answer("pane get w1:p3", &Value::Obj(vec![("pane".into(), term.clone())]));
    let focus = json::parse(r#"{"focus":{"focused_pane_id":"w1:p3"}}"#).unwrap();
    f.answer("pane focus --direction right --pane w1:p2", &focus);
    assert_eq!(actions::insert("/src/a b.txt", "w1:p2"), None);
    assert!(f.called("pane send-text w1:p3 'a b.txt' "));
    assert!(f.called("pane focus --direction right --pane w1:p2"));
    // An agent: @ before a path without spaces; quoted with them.
    term.set("agent", Value::Str("claude".into()));
    f.answer("pane get w1:p3", &Value::Obj(vec![("pane".into(), term)]));
    assert_eq!(actions::insert("/src/lib/x.rs", "w1:p2"), None);
    assert!(f.called("pane send-text w1:p3 @lib/x.rs "));
    assert_eq!(actions::insert("/src/a\nb", "w1:p2").as_deref(), Some("Not inserted: control character in name"));
    assert_eq!(actions::type_text(Line::Of(&|cwd| format!("cd {}", cwd.unwrap_or("?"))), "w1:p2"), None);
    assert!(f.called("pane send-text w1:p3 cd /src"));
    assert_eq!(actions::type_text(Line::Text("a\x1bb"), "w1:p2").as_deref(), Some("Not typed: control character"));
    // The plugin's own panes are passed over; none left: no terminal.
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", Some("Preview"), "w1:t1"),
        pane("w1:p5", Some("Changes"), "w1:t1"),
    ]);
    assert_eq!(actions::terminal_of("w1:p2"), (None, None));
    assert_eq!(actions::insert("/x", "w1:p2").as_deref(), Some("No terminal next to the explorer"));
    f.reset(&[
        pane("w1:p2", Some("Explorer"), "w1:t1"),
        pane("w1:p3", Some("Preview"), "w1:t1"),
        pane("w1:p5", None, "w1:t1"),
    ]);
    assert_eq!(actions::terminal_of("w1:p2"), (Some("w1:p5".into()), Some("right")));
}

/// A layout of two panes side by side in a split of split columns, the explorer (p2) its left part at ratio.
fn layout(split: f64, ratio: f64, area: f64) -> Value {
    let w = (split * ratio).round();
    json::parse(&format!(
        r#"{{"area":{{"width":{area}}},"zoomed":false,
        "panes":[{{"pane_id":"w1:p2","rect":{{"x":0,"y":0,"width":{w},"height":40}}}},
                 {{"pane_id":"w1:p3","rect":{{"x":{w},"y":0,"width":{},"height":40}}}}],
        "splits":[{{"direction":"right","ratio":{ratio},"rect":{{"x":0,"y":0,"width":{split},"height":40}}}}]}}"#,
        split - w
    ))
    .unwrap()
}

struct Width {
    columns: f64,
    kept: Option<f64>,
}

impl Kept for Width {
    fn columns(&self) -> f64 {
        self.columns
    }
    fn get(&self) -> Option<f64> {
        self.kept
    }
    fn set(&mut self, width: f64) {
        self.kept = Some(width);
    }
}

#[test]
fn a_pane_keeps_its_width_and_takes_the_one_dragged_to() {
    let f = Fake::new();
    f.reset(&[]);
    let lay = layout(100.0, 0.3, 100.0);
    assert_eq!(
        actions::side_split(&lay, "w1:p2"),
        Some(actions::Found { width: 30.0, split_width: 100.0, ratio: 0.3, left: true })
    );
    assert_eq!(actions::side_split(&lay, "w1:p3").map(|s| s.left), Some(false));
    assert_eq!(actions::side_split(&lay, "w1:p9"), None);

    let mut keeper = WidthKeeper::new("w1:p2", Some("left"));
    let mut w = Width { columns: 29.0, kept: None };
    let at = |l: Value| f.answer("pane layout --pane w1:p2", &Value::Obj(vec![("layout".into(), l)]));
    at(layout(100.0, 0.3, 100.0));
    // At first, the width it has (border: 1 column).
    keeper.check(false, &mut w);
    assert_eq!(w.kept, Some(29.0));
    assert_eq!(keeper.left(), Some(true));
    // The window widened: the split kept its ratio, the pane is put back to 29 columns (30 with its border).
    at(layout(120.0, 0.3, 120.0));
    w.columns = 35.0;
    keeper.check(false, &mut w);
    assert_eq!(w.kept, Some(29.0));
    assert!(f.called("pane resize --pane w1:p2 --direction left --amount 0.047917"));
    // Its own border dragged (the ratio changed, nothing else): the new width is kept.
    at(layout(120.0, 0.4, 120.0));
    w.columns = 47.0;
    keeper.check(false, &mut w);
    assert_eq!(w.kept, Some(47.0));
    // Zoomed: nothing done.
    let mut zoomed = layout(120.0, 0.5, 120.0);
    zoomed.set("zoomed", Value::Bool(true));
    at(zoomed);
    keeper.check(true, &mut w);
    assert_eq!(w.kept, Some(47.0));
    at(layout(120.0, 0.4, 120.0));
    // A side asked that the pane is not on: nothing done.
    let mut right = WidthKeeper::new("w1:p2", Some("right"));
    let mut v = Width { columns: 39.0, kept: None };
    right.check(false, &mut v);
    assert_eq!((v.kept, right.left()), (None, None));
}

#[test]
fn a_pane_of_its_own_tab_shares_its_width_as_the_settings_say() {
    let f = Fake::new();
    f.reset(&[]);
    f.answer("pane layout --pane w1:p4", &Value::Obj(vec![("layout".into(), layout(100.0, 0.3, 100.0).replace_ids())]));
    let mut sharer = actions::Sharer::new("w1:p4", "scmShare");
    sharer.share(0.0);
    // 35 % at first: moved right by 0.05.
    let resize = f.calls().into_iter().find(|a| a.get(1).map(String::as_str) == Some("resize")).unwrap();
    assert_eq!(resize[2..].join(" "), "--pane w1:p4 --direction right --amount 0.050000");
    let settings = json::parse(&exec::read_text(f.state.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings.get("scmShare"), Some(&Value::Num(0.3)));
}

trait Ids {
    fn replace_ids(self) -> Value;
}

impl Ids for Value {
    fn replace_ids(self) -> Value {
        json::parse(&json::stringify(&self).replace("w1:p2", "w1:p4")).unwrap()
    }
}

#[test]
fn a_tab_of_its_own_is_recorded_named_and_shown_then_handed_back_at_the_exit() {
    let f = Fake::new();
    f.reset(&[pane("w1:p7", Some("Source Control"), "w1:t2"), pane("w1:p8", Some("Preview"), "w1:t2")]);
    // SAFETY: under LOCK.
    unsafe { std::env::set_var("HERDR_PANE_ID", "w1:p7") };
    f.answer("tab get w1:t1", &json::parse(r#"{"tab":{"label":"agent"}}"#).unwrap());
    f.answer("workspace list", &json::parse(r#"{"workspaces":[{"focused":true,"active_tab_id":"w1:t2"}]}"#).unwrap());
    actions::hold_tab("review", "w1:t1", "Review");
    let record = f.state.join("review-w1_t1.json");
    assert_eq!(std::fs::read_to_string(&record).unwrap(), r#"{"pane":"w1:p7"}"#);
    for _ in 0..300 {
        if f.called("tab focus w1:t2") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(f.called("tab rename w1:t2 (Review agent)"));
    assert!(f.called("tab focus w1:t2"));
    exec::exit_hooks();
    assert!(!record.exists());
    assert!(f.called("tab focus w1:t1"));
    assert_eq!(f.closed(), ["w1:p8"]);
    unsafe { std::env::remove_var("HERDR_PANE_ID") };
}
