//! The `peek` subcommand (src/peek.js): shows a file in the preview of the tab it runs in, for an agent, a script or a
//! key binding.
//!   peek <file>[:<places>] the file, scrolled to the first place and the places highlighted: 12, 12-14, 12:5-18
//!                          (columns of line 12), several separated by commas (12-14,30,42:5-18)
//!   peek --last            the changed file of the repository written last (the peek action)
//!   peek --diff …          the same, showing the file's git changes (the preview's Diff view) at the places
//!   peek --url             the link clicked in a terminal (HERDR_PLUGIN_CLICKED_URL, the open-link action): a
//!                          file:// URL, or a path[:places] the terminal's folder resolves
//!
//! Run from a herdr pane (an agent's terminal): the preview of that pane's tab shows the file, opened next to it when
//! there is none. Prints what it showed, or why not, with exit code 1 (2: wrong use).

use crate::core::actions::{self, Shown};
use crate::core::exec::{self, OWN_PANES, Opts};
use crate::core::places::{self, Mark};
use crate::core::{icons, js, paths};
use crate::json::{self, Value};

/// Why peek stops: the message, after "peek: ", and the exit code.
struct Fail(String, i32);

fn fail<T>(text: impl Into<String>, code: i32) -> Result<T, Fail> {
    Err(Fail(text.into(), code))
}

/// A herdr command's result, null when it fails.
fn herdr(args: &[&str]) -> Value {
    let out = exec::exec(exec::herdr(), args, &Opts::default());
    if out.code != Some(0) {
        return Value::Null;
    }
    json::parse(&out.out).ok().and_then(|v| v.get("result").cloned()).unwrap_or(Value::Null)
}

/// Runs peek: its exit code.
pub fn run(args: Vec<String>) -> i32 {
    match peek(args) {
        Ok(()) => 0,
        Err(Fail(text, code)) => {
            eprintln!("peek: {text}");
            code
        }
    }
}

/// The plugin's state folder, where the preview reads its requests: herdr gives it to the plugin's own processes
/// only. From another pane, the one the hook saved in the plugin's config folder (state-dir), else herdr's usual
/// place.
fn find_state_dir() -> Result<(), Fail> {
    if exec::state_dir().is_some() {
        return Ok(());
    }
    let id = exec::plugin();
    let out = exec::exec(exec::herdr(), &["plugin", "config-dir", &id], &Opts::default());
    let config = if out.code == Some(0) { out.out.trim_matches(js::is_space).to_string() } else { String::new() };
    let mut dir = std::fs::read_to_string(paths::join(&[&config, "state-dir"]))
        .map(|d| d.trim_matches(js::is_space).to_string())
        .unwrap_or_default();
    if dir.is_empty() {
        let base = if exec::WIN {
            std::env::var("LOCALAPPDATA").unwrap_or_default()
        } else {
            exec::env_var("XDG_STATE_HOME").unwrap_or_else(|| paths::join(&[&icons::homedir(), ".local", "state"]))
        };
        dir = paths::join(&[&base, "herdr", "plugins", &id]);
    }
    if std::fs::metadata(&dir).is_err() {
        return fail("cannot find the plugin's state folder: open a new tab once so that the explorer saves it", 1);
    }
    // SAFETY: peek runs on one thread, and nothing has read the environment from another.
    unsafe { std::env::set_var("HERDR_PLUGIN_STATE_DIR", &dir) };
    Ok(())
}

/// The pane it runs from, and its folder. As the peek action, the tab's focused pane, or its first terminal when the
/// focus is on one of the plugin's panes.
fn origin() -> (Option<String>, String) {
    let mut pane = exec::env_var("HERDR_PANE_ID").map(Value::Str);
    let mut cwd = paths::cwd();
    if let Some(text) = exec::env_var("HERDR_PLUGIN_CONTEXT_JSON") {
        let ctx = json::parse(&text).unwrap_or(Value::Obj(Vec::new()));
        let own = |p: &Value| p.get("label").and_then(Value::as_str).is_some_and(|l| OWN_PANES.contains(&l));
        // An answer without its panes, on which the JS threw, counts as none.
        let list = match ctx.get("workspace_id").filter(|w| w.truthy()) {
            Some(ws) => herdr(&["pane", "list", "--workspace", &js::string(ws)]),
            None => Value::Null,
        };
        let all = list.get("panes").and_then(Value::as_array).cloned().unwrap_or_default();
        let panes: Vec<Value> = all.into_iter().filter(|p| js::strict_eq(p.get("tab_id"), ctx.get("tab_id"))).collect();
        let focused = panes.iter().find(|p| js::strict_eq(p.get("pane_id"), ctx.get("focused_pane_id")));
        let terminal = focused.filter(|f| !own(f)).or_else(|| panes.iter().find(|p| !own(p)));
        pane = match terminal {
            Some(t) => t.get("pane_id").cloned(),
            None => ctx.get("focused_pane_id").cloned(),
        };
        let found = terminal.and_then(|t| t.get("cwd")).filter(|c| c.truthy());
        if let Some(c) = found.or_else(|| ctx.get("workspace_cwd").filter(|c| c.truthy())) {
            cwd = js::string(c);
        }
    }
    (pane.filter(Value::truthy).map(|p| js::string(&p)), cwd)
}

/// The changed file of the repository written last: modified, added or untracked, still on disk.
fn last(cwd: &str) -> Result<String, Fail> {
    let env = exec::git_env(cwd);
    let opts = Opts { cwd: Some(cwd.into()), env: Some(env.clone()), ..Opts::default() };
    let top = exec::run(exec::git(), &["rev-parse", "--show-toplevel"], &opts).unwrap_or_default();
    let top = top.trim_matches(js::is_space);
    if top.is_empty() {
        return fail("not in a git repository", 1);
    }
    let opts = Opts { cwd: Some(top.into()), env: Some(env), ..Opts::default() };
    let args = ["status", "--porcelain=v1", "-z", "--untracked-files=all"];
    let Some(out) = exec::run(exec::git(), &args, &opts) else { return fail("git status failed", 1) };
    let mut best: Option<(String, f64)> = None;
    let parts: Vec<&str> = out.split('\0').collect();
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        i += 1;
        if p.encode_utf16().count() < 4 {
            continue;
        }
        // The original path follows (renamed in the index or the tree).
        let b = p.as_bytes();
        if b"RC".contains(&b[0]) || b"RC".contains(&b[1]) {
            i += 1;
        }
        let abs = paths::join(&[top, p.get(3..).unwrap_or("")]);
        if exec::remote_link(&abs, top) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&abs) else { continue };
        let time = meta.modified().map_or(f64::NAN, js::time_ms);
        if meta.is_file() && best.as_ref().is_none_or(|b| time > b.1) {
            best = Some((abs, time));
        }
    }
    match best {
        Some((abs, _)) => Ok(abs),
        None => fail("no changed file", 1),
    }
}

fn mark_value(m: &Mark) -> Value {
    let n = |x: u64| Value::Num(x as f64);
    Value::Obj(vec![("from".into(), n(m.from)), ("to".into(), n(m.to)), ("a".into(), n(m.a)), ("b".into(), n(m.b))])
}

fn peek(mut args: Vec<String>) -> Result<(), Fail> {
    // Found in the absolute folders of PATH: on Windows a bare name would run a herdr.exe of the working directory.
    if exec::herdr().is_none() {
        return fail("herdr not found in PATH", 1);
    }
    find_state_dir()?;
    let (pane, cwd) = origin();
    let Some(pane) = pane else { return fail("run it from a herdr pane", 2) };

    // --diff: the file's git changes, at the places, rather than the file.
    let diff = args.first().is_some_and(|a| a == "--diff");
    if diff {
        args.remove(0);
    }
    if args.len() == 1 && args[0] == "--url" {
        let url = exec::env_var("HERDR_PLUGIN_CLICKED_URL");
        let Some(arg) = places::clicked(url.as_deref().unwrap_or(""), &crate::sys::hostname()) else {
            return fail(format!("not a local file: {}", url.as_deref().unwrap_or("(no link)")), 1);
        };
        args[0] = arg;
    }
    if args.len() != 1 || args[0] == "--help" || args[0] == "-h" {
        return fail("usage: peek.js [--diff] <file>[:<line>[-<line>][:<col>-<col>],…] | [--diff] --last | --url", 2);
    }
    let (abs, marks) = if args[0] == "--last" {
        (last(&cwd)?, Vec::new())
    } else {
        match places::target(&args[0], &cwd) {
            Ok(t) => t,
            Err(e) => return fail(e, 2),
        }
    };
    let line = marks.first().map_or(0.0, |m| m.from as f64);
    // A deleted file still has changes to show.
    if exec::remote_link(&abs, "") {
        return fail(format!("link to a network share, not shown: {abs}"), 1);
    }
    match std::fs::metadata(&abs) {
        Ok(meta) if !meta.is_file() => return fail(format!("not a file: {abs}"), 1),
        Ok(_) => {}
        Err(_) if !diff => return fail(format!("no such file: {abs}"), 1),
        Err(_) => {}
    }
    let o = Shown {
        marks: marks.iter().map(mark_value).collect(),
        view: if diff { "diff" } else { "file" }.into(),
        ..Shown::default()
    };
    if let Some(err) = actions::preview(&abs, &cwd, &pane, line, false, true, &o) {
        return fail(err, 1);
    }
    let places: Vec<String> = marks
        .iter()
        .map(|m| {
            let to = if m.to > m.from { format!("-{}", m.to) } else { String::new() };
            let cols = if m.a != 0 { format!(":{}-{}", m.a, m.b) } else { String::new() };
            format!("{}{to}{cols}", m.from)
        })
        .collect();
    let places = places.join(",");
    let places = if places.is_empty() { places } else { format!(":{places}") };
    println!("Preview{}: {}{places}", if diff { " (diff)" } else { "" }, actions::relative_to(Some(&cwd), &abs));
    Ok(())
}
