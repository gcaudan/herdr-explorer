//! The `hook` subcommand: opens the explorer to the left (or the right, depending on the settings) of a
//! tab.
//!   hook           tab.created event (creating a space also emits it for its first tab), unless the autoOpen
//!                  setting is off
//!   hook --toggle  key binding action: opens the current tab's explorer, closes it when it is open
//!   hook --restore startup hook: reopens the explorer and preview panes that herdr restored as shells
//!   hook --search  key binding action: Go to file popup of the current tab's folder
//!   hook --scm     key binding action: source control of the current tab's folder
//!   hook --closed  tab.closed and workspace.closed events: removes the state of the tabs and panes gone
//!   hook --agent   pane.agent_status_changed event: the working tree when the agent starts a turn (scope)
//!   hook --focused tab.focused event: the tab shown noted (core::shown); a tab under review leads to its review
//!                  (actions::scm)
//!
//! Each run also saves the state folder for peek. Where herdr fails or an answer lacks what it reads, the hook
//! stops with code 1, without the stack.

use crate::core::actions::{self, Shown};
use crate::core::exec::{self, OWN_PANES};
use crate::core::{paths, scope, shown, update};
use crate::json::{self, Value};

const LABEL: &str = "Explorer";

/// A failure: the hook stops there, code 1.
struct Thrown;

type R<T> = Result<T, Thrown>;

fn some<T>(v: Option<T>) -> R<T> {
    v.ok_or(Thrown)
}

/// A herdr command's result, waited for: Thrown when herdr fails.
fn herdr(args: &[&str]) -> R<Value> {
    let out = exec::exec(exec::herdr(), args, &exec::Opts::default());
    if out.code != Some(0) {
        return Err(Thrown);
    }
    Ok(exec::result(Some(&out.out)))
}

/// A field as text for a herdr argument, "undefined" when missing.
fn str_of(v: &Value, key: &str) -> String {
    v.get(key).map_or_else(|| "undefined".into(), json::text)
}

/// `v.key` of an array, thrown when it is not one (`.filter`, `.find` on it).
fn list(v: &Value, key: &str) -> R<Vec<Value>> {
    some(v.get(key).and_then(Value::as_array).cloned())
}

fn label_is(p: &Value, label: &str) -> bool {
    p.get("label").and_then(Value::as_str) == Some(label)
}

/// One of the plugin's own panes (OWN_PANES.has(p.label)).
fn own(p: &Value) -> bool {
    p.get("label").and_then(Value::as_str).is_some_and(|l| OWN_PANES.contains(&l))
}

fn workspace_panes(workspace: &str) -> R<Vec<Value>> {
    list(&herdr(&["pane", "list", "--workspace", workspace])?, "panes")
}

fn tab_panes(workspace: &str, tab: Option<&Value>) -> R<Vec<Value>> {
    Ok(workspace_panes(workspace)?.into_iter().filter(|p| p.get("tab_id") == tab).collect())
}

fn workspaces() -> R<Vec<Value>> {
    list(&herdr(&["workspace", "list"])?, "workspaces")
}

/// What the plugin keeps for tabs and panes that no longer exist (notes, pane state, preview tabs), removed.
fn sweep() -> R<()> {
    let mut panes = Vec::new();
    for ws in workspaces()? {
        panes.extend(workspace_panes(&str_of(&ws, "workspace_id"))?);
    }
    let mut tabs: Vec<String> = Vec::new();
    for p in &panes {
        let t = str_of(p, "tab_id");
        if !tabs.contains(&t) {
            tabs.push(t);
        }
    }
    let ids: Vec<String> = panes.iter().map(|p| str_of(p, "pane_id")).collect();
    exec::sweep_state(&tabs, &ids);
    Ok(())
}

fn focused_tab() -> R<Option<Value>> {
    let ws = workspaces()?.into_iter().find(|w| w.get("focused").is_some_and(Value::truthy));
    Ok(ws.and_then(|w| w.get("active_tab_id").cloned()))
}

fn right() -> bool {
    exec::read_settings().get("side").and_then(Value::as_str) == Some("right")
}

/// Math.min(hi, Math.max(lo, x)), NaN kept.
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() { x } else { x.max(lo).min(hi) }
}

fn rect(r: &Value, key: &str) -> f64 {
    json::num(r.get("rect").and_then(|r| r.get(key)))
}

/// Opens the explorer in the tab; tree: the tree root, when Set as root had moved it away from the space folder
/// before a herdr restart. Gives the explorer's pane.
fn open(panes: &[Value], tab: &Value, root: Option<String>, tree: Option<String>) -> R<String> {
    let right = right();
    let first = some(panes.first())?;
    // The top-left pane: the explorer takes its place and pushes it to the right. Docked on the right: the top-right
    // pane, which the explorer splits.
    let layout = some(herdr(&["pane", "layout", "--pane", &str_of(first, "pane_id")])?.get("layout").cloned())?;
    let area = json::num(layout.get("area").and_then(|a| a.get("width")));
    let corner = list(&layout, "panes")?.into_iter().find(|r| {
        rect(r, "y") == 0.0 && if right { rect(r, "x") + rect(r, "width") == area } else { rect(r, "x") == 0.0 }
    });
    let target =
        corner.as_ref().and_then(|c| panes.iter().find(|p| p.get("pane_id") == c.get("pane_id"))).unwrap_or(first);
    let target_id = str_of(target, "pane_id");
    let before = focused_tab()?;

    let root = root.unwrap_or_else(|| str_of(target, "cwd"));
    let plugin = exec::plugin();
    let mut args = vec!["plugin", "pane", "open", "--plugin", &plugin, "--entrypoint", "tree"];
    args.extend(["--placement", "split", "--target-pane", &target_id, "--direction", "right"]);
    let root_env = format!("EXPLORER_ROOT={root}");
    args.extend(["--env", &root_env]);
    let tree_env = tree.map(|t| format!("EXPLORER_TREE={t}"));
    if let Some(t) = &tree_env {
        args.extend(["--env", t]);
    }
    args.push("--no-focus");
    let opened = herdr(&args)?;
    let explorer = some(opened.get("plugin_pane").and_then(|p| p.get("pane")).and_then(|p| p.get("pane_id")))?;
    let explorer = json::text(explorer);

    // The split starts half-and-half of the target pane's space: set it right away to the wanted width (including
    // the 2-column border), without waiting for the explorer to start, which then corrects any rounding.
    // The ratio belongs to the left slot, which the explorer takes over on the swap.
    if let Some(corner) = &corner {
        let part = clamp((exec::read_width() + 2.0) / rect(corner, "width"), 0.1, 0.9);
        let delta = (if right { 1.0 - part } else { part }) - 0.5;
        let amount = format!("{:.6}", delta.abs());
        let direction = if delta > 0.0 { "right" } else { "left" };
        let _ = herdr(&["pane", "resize", "--pane", &target_id, "--direction", direction, "--amount", &amount]);
    }

    // herdr only splits to the right or downwards: swap to move the explorer to the left.
    // The swap leaves focus on the left, so give it back to the terminal, which displays this tab;
    // then return to the tab the user was viewing.
    if !right {
        herdr(&["pane", "swap", "--source-pane", &explorer, "--target-pane", &target_id])?;
    }
    let _ = settle_preview(panes, tab, &explorer);
    herdr(&["pane", "focus", "--direction", if right { "left" } else { "right" }, "--pane", &explorer])?;
    if let Some(before) = before.filter(|b| b.truthy() && b != tab) {
        herdr(&["tab", "focus", &json::text(&before)])?;
    }
    Ok(explorer)
}

/// Opened in a tab that already has a preview, the explorer shares a split with the pane it split, on one side of the
/// split whose other side is the preview: dragging the preview's border then resizes the explorer too, herdr keeping
/// each split's ratio. The preview moved out of the tab and back beside the terminal (herdr does not move a pane
/// within its tab), at the same width, leaves the explorer a split of its own, at the side of the tab.
fn settle_preview(panes: &[Value], tab: &Value, explorer: &str) -> R<()> {
    let preview = panes.iter().find(|p| label_is(p, "Preview"));
    let terminals: Vec<&Value> = panes.iter().filter(|p| !p.get("label").is_some_and(Value::truthy)).collect();
    let (Some(preview), [terminal]) = (preview, terminals.as_slice()) else { return Ok(()) };
    let layout = some(herdr(&["pane", "layout", "--pane", explorer])?.get("layout").cloned())?;
    let Some(found) = actions::side_split(&layout, explorer) else { return Ok(()) };
    if found.split_width == json::num(some(layout.get("area"))?.get("width")) {
        return Ok(());
    }
    let rects = list(&layout, "panes")?;
    let find = |p: &Value| rects.iter().find(|r| r.get("pane_id") == p.get("pane_id")).cloned();
    let (Some(p), Some(t)) = (find(preview), find(terminal)) else { return Ok(()) };
    let id = str_of(preview, "pane_id");
    herdr(&["pane", "move", &id, "--new-tab", "--no-focus"])?;
    let ratio = format!("{:.4}", rect(&t, "width") / (rect(&t, "width") + rect(&p, "width")));
    let (tab, target) = (json::text(tab), str_of(terminal, "pane_id"));
    let args = ["pane", "move", &id, "--tab", &tab, "--target-pane", &target, "--split", "right", "--ratio", &ratio];
    herdr(&[&args[..], &["--no-focus"]].concat())?;
    Ok(())
}

/// The JSON of a variable, `{}` when unset; thrown when it does not parse.
fn env_json(name: &str) -> R<Value> {
    let text = exec::env_var(name).unwrap_or_else(|| "{}".into());
    json::parse(&text).map_err(|_| Thrown)
}

/// The state folder, saved in the config folder for peek, which runs outside the plugin (from an agent's pane) and
/// can find the config folder only (herdr plugin config-dir).
fn save_state_dir() {
    let (Some(state), Some(config)) = (exec::state_dir(), exec::config_dir()) else { return };
    let file = paths::join(&[&config, "state-dir"]);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(state.as_str()) {
        let _ = std::fs::create_dir_all(&config).and_then(|_| std::fs::write(&file, &state));
    }
}

/// Runs the hook: its exit code.
pub fn run(args: &[String]) -> i32 {
    if exec::herdr().is_none() {
        return 0;
    }
    save_state_dir();
    match hook(args) {
        Ok(()) => 0,
        Err(Thrown) => 1,
    }
}

fn hook(args: &[String]) -> R<()> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let ctx = env_json("HERDR_PLUGIN_CONTEXT_JSON")?;
    let ws = str_of(&ctx, "workspace_id");
    let cwd_of = |v: &Value| v.get("workspace_cwd").filter(|c| c.truthy()).map(json::text);

    if has("--toggle") {
        let Some(tab) = ctx.get("tab_id").filter(|t| t.truthy()) else { return Ok(()) };
        // The action's tab is the one shown (no tab.focused event after herdr starts).
        shown::set(&json::text(tab));
        let panes = tab_panes(&ws, Some(tab))?;
        if let Some(explorer) = panes.iter().find(|p| label_is(p, LABEL)) {
            // The preview closes with the explorer (see close in the explorer).
            for p in panes.iter().filter(|p| label_is(p, "Preview")) {
                herdr(&["pane", "close", &str_of(p, "pane_id")])?;
            }
            herdr(&["pane", "close", &str_of(explorer, "pane_id")])?;
        } else if panes.iter().any(|p| !own(p)) {
            open(&panes, tab, cwd_of(&ctx), None)?;
        }
    } else if has("--search") || has("--scm") {
        // Any failure ignored here.
        let _ = search(&ctx, has("--scm"));
    } else if has("--focused") {
        let ev = env_json("HERDR_PLUGIN_EVENT_JSON")?;
        let d = ev.get("data").filter(|d| d.truthy()).cloned().unwrap_or(Value::Obj(Vec::new()));
        let tab = d.get("tab_id").filter(|t| t.truthy()).or_else(|| d.get("tab").and_then(|t| t.get("tab_id")));
        if let Some(tab) = tab.filter(|t| t.truthy()).map(json::text) {
            // Follow wakes in the explorer of that tab only.
            shown::set(&tab);
            if let Some(p) = actions::review_of(&tab) {
                actions::focus_pane(&p);
            }
        }
    } else if has("--closed") {
        sweep()?;
    } else if has("--agent") {
        let ev = env_json("HERDR_PLUGIN_EVENT_JSON")?;
        let d = ev.get("data").cloned().unwrap_or(Value::Null);
        let pane = d
            .get("pane_id")
            .filter(|p| p.truthy())
            .and_then(|id| herdr(&["pane", "get", &json::text(id)]).ok())
            .and_then(|r| r.get("pane").cloned())
            .filter(Value::truthy);
        if let Some(pane) = pane
            && pane.get("tab_id").is_some_and(Value::truthy)
            && pane.get("cwd").is_some_and(Value::truthy)
        {
            let status = str_of(&d, "agent_status");
            let (tab, id, cwd) = (str_of(&pane, "tab_id"), str_of(&pane, "pane_id"), str_of(&pane, "cwd"));
            println!("{}", scope::agent_status(&tab, &id, &status, &cwd));
        }
    } else if has("--restore") {
        // A newer release, looked for alongside (at most once a day).
        let check = std::thread::spawn(update::check);
        restore()?;
        let _ = check.join();
    } else {
        // Also here, for a herdr session running for days: the check itself happens once a day at most.
        let check = std::thread::spawn(update::check);
        let ev = env_json("HERDR_PLUGIN_EVENT_JSON")?;
        let tab = ev.get("data").and_then(|d| d.get("tab")).filter(|t| t.truthy());
        if let Some(tab) = tab
            && exec::read_settings().get("autoOpen").is_some_and(Value::truthy)
        {
            let panes = tab_panes(&str_of(tab, "workspace_id"), tab.get("tab_id"))?;
            let root = if ctx.get("workspace_id") == tab.get("workspace_id") { cwd_of(&ctx) } else { None };
            // Not in the editor's tab, opened as a tab of its own.
            if panes.len() == 1 && !own(&panes[0]) {
                open(&panes, tab.get("tab_id").unwrap_or(&Value::Null), root, None)?;
            }
        }
        let _ = check.join();
    }
    Ok(())
}

/// --search and --scm: from the tab's explorer when there is one, so that the search opens files in that tab's
/// preview.
fn search(ctx: &Value, scm: bool) -> R<()> {
    // From a tab of their own (source control, the search), the tab they were opened from (actions::origin_of).
    let here = exec::env_var("HERDR_PANE_ID").map(Value::Str).or_else(|| ctx.get("pane_id").cloned());
    let here = here.filter(Value::truthy);
    let from = here.as_ref().and_then(|h| actions::origin_of(&json::text(h)));
    let tab = match &from {
        Some((_, tab)) => Some(Value::Str(tab.clone())),
        None => ctx.get("tab_id").cloned(),
    };
    let panes = if tab.as_ref().is_some_and(Value::truthy) {
        tab_panes(&str_of(ctx, "workspace_id"), tab.as_ref())?
    } else {
        vec![]
    };
    let explorer = panes.iter().find(|p| label_is(p, LABEL));
    let pane = explorer
        .and_then(|e| e.get("pane_id").filter(|p| p.truthy()).cloned())
        .or_else(|| match &from {
            Some((pane, _)) => Some(Value::Str(pane.clone())),
            None => here.clone(),
        })
        .filter(Value::truthy)
        .or_else(|| panes.first().and_then(|p| p.get("pane_id").cloned()));
    // The explorer's root: workspace_cwd is the folder of the focused pane, the plugin's own when it is one of its
    // panes.
    let saved = explorer.map(|e| exec::read_pane_state(&str_of(e, "pane_id"))).unwrap_or(Value::Null);
    // The focused terminal first, else the tab's first one.
    let focused = panes.iter().find(|p| p.get("pane_id") == here.as_ref());
    let terminal = focused.filter(|f| !own(f)).or_else(|| panes.iter().find(|p| !own(p)));
    let root = saved
        .get("root")
        .filter(|r| r.truthy())
        .or_else(|| terminal.and_then(|t| t.get("cwd")).filter(|c| c.truthy()))
        .or_else(|| ctx.get("workspace_cwd"))
        .filter(|r| r.truthy());
    if let (Some(pane), Some(root)) = (pane.filter(Value::truthy), root) {
        let (pane, root) = (json::text(&pane), json::text(root));
        if scm {
            actions::scm(&root, &pane, "", false);
        } else {
            actions::search(&root, &pane, "files", "");
        }
    }
    Ok(())
}

/// The path when it is a folder (dir) or something else, else None.
fn exists(p: Option<&Value>, dir: bool) -> Option<String> {
    let Some(Value::Str(p)) = p else { return None };
    let meta = std::fs::metadata(p).ok()?;
    (!p.is_empty() && meta.is_dir() == dir).then(|| p.clone())
}

/// A pane still running its program, this binary (herdr restored the others as shells).
fn running(p: &Value) -> R<bool> {
    let info = herdr(&["pane", "process-info", "--pane", &str_of(p, "pane_id")])?;
    let procs = list(some(info.get("process_info"))?, "foreground_processes")?;
    Ok(procs.iter().any(|f| {
        let argv0 = f
            .get("argv")
            .filter(|a| a.truthy())
            .and_then(|a| a.as_array().and_then(|a| a.first()))
            .filter(|a| a.truthy());
        let name = match argv0 {
            None => String::new(),
            Some(Value::Str(s)) => paths::basename(s, None),
            Some(_) => return false,
        };
        let name = name.to_lowercase();
        ["herdr-explorer", "herdr-explorer.exe"].contains(&name.as_str())
    }))
}

/// herdr restores the plugin's panes as plain shells, its session keeping only their label and folder: they are
/// closed, then the explorer and the preview reopened in the tabs that had them, at what they saved under the same
/// pane id (the preview's file only when it still exists).
fn restore() -> R<()> {
    for ws in workspaces()? {
        let mut tabs: Vec<(Value, Vec<Value>)> = Vec::new();
        for p in workspace_panes(&str_of(&ws, "workspace_id"))? {
            let tab = p.get("tab_id").cloned().unwrap_or(Value::Null);
            match tabs.iter_mut().find(|(t, _)| *t == tab) {
                Some((_, panes)) => panes.push(p),
                None => tabs.push((tab, vec![p])),
            }
        }
        for (tab, panes) in tabs {
            let mut stale = Vec::new();
            let mut rest = Vec::new();
            for p in panes {
                if own(&p) && !running(&p)? { stale.push(p) } else { rest.push(p) }
            }
            if stale.is_empty() {
                continue;
            }
            for p in &stale {
                herdr(&["pane", "close", &str_of(p, "pane_id")])?;
            }
            let Some(first) = rest.first() else { continue };
            let mut anchor = str_of(first, "pane_id");
            if let Some(explorer) = stale.iter().find(|p| label_is(p, LABEL)) {
                let saved = exec::read_pane_state(&str_of(explorer, "pane_id"));
                let home = exists(saved.get("home"), true);
                let tree = home.as_ref().and_then(|_| exists(saved.get("root"), true));
                anchor = open(&rest, &tab, home, tree)?;
            }
            // The preview's tabs whose file still exists, the one shown first among them.
            let shown =
                stale.iter().find(|p| label_is(p, "Preview")).map(|p| exec::read_pane_state(&str_of(p, "pane_id")));
            let shown = shown.filter(Value::truthy);
            let saved = shown
                .as_ref()
                .and_then(|s| s.get("tabs"))
                .filter(|t| t.truthy() && matches!(t.get("list"), Some(Value::Arr(_))));
            let (mut list, mut at) = (Vec::new(), -1);
            if let Some(saved) = saved {
                let all = saved.get("list").and_then(Value::as_array).cloned().unwrap_or_default();
                let current = saved.get("current").map(json::text);
                for (i, t) in all.into_iter().enumerate() {
                    if t.truthy() && exists(t.get("file"), false).is_some() {
                        if current.as_deref() == Some(i.to_string().as_str()) {
                            at = list.len() as i64;
                        }
                        list.push(t);
                    }
                }
            }
            let first = if at >= 0 {
                Some(list[at as usize].clone())
            } else {
                list.first().cloned().or_else(|| shown.filter(|s| exists(s.get("file"), false).is_some()))
            };
            if let Some(first) = first {
                let tabs = (!list.is_empty()).then(|| {
                    Value::Obj(vec![
                        ("list".into(), Value::Arr(list.clone())),
                        ("current".into(), Value::Num(at.max(0) as f64)),
                    ])
                });
                let o = Shown {
                    view: if first.get("view").and_then(Value::as_str) == Some("diff") { "diff" } else { "file" }
                        .into(),
                    keep: first.get("kept").is_some_and(Value::truthy),
                    tabs,
                    ..Shown::default()
                };
                let root = first.get("root").map_or_else(String::new, json::text);
                // showPreview's default 0 for a tab saved without its line; what the preview reads as 0 too.
                let line = first.get("line").map_or(0.0, |l| json::num(Some(l)));
                let line = if line.is_nan() { 0.0 } else { line };
                actions::preview(
                    &json::text(first.get("file").unwrap_or(&Value::Null)),
                    &root,
                    &anchor,
                    line,
                    false,
                    true,
                    &o,
                );
            }
        }
    }
    // The state of tabs and panes that no longer exist, closed while the plugin was off or before a restart.
    sweep()
}
