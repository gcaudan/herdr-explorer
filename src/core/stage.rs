//! Staging, unstaging and discarding parts of a diff, from the Changes pane and the preview's Diff view: the rows
//! selected give whole files, hunks or lines, and a patch made from the parsed diff (diff::parse) goes through
//! git apply: --cached for the index, -R to take changes back.
//!   stage    lines of the Unstaged section, added to the index
//!   unstage  lines of the Staged section, taken out of the index
//!   discard  lines of the Unstaged section, taken out of the working tree (the caller confirms first)
//! Files, hunks and lines are named by their place in the diff: file index, (file, hunk), (file, hunk, line).

use super::exec::{Env, Exec, Opts, WIN, env_set, exec, git, git_env, io_code, walk_realpath};
use super::paths;
use crate::diff::{self, Diff, Line};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Stage,
    Unstage,
    Discard,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Stage => "stage",
            Mode::Unstage => "unstage",
            Mode::Discard => "discard",
        }
    }

    /// The section each mode works on.
    pub fn section(self) -> &'static str {
        match self {
            Mode::Unstage => "staged",
            Mode::Stage | Mode::Discard => "unstaged",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Mode::Stage => "Staged",
            Mode::Unstage => "Unstaged",
            Mode::Discard => "Discarded",
        }
    }
}

/// What rows select: whole files from their name's row, whole hunks from their @@ row, lines from theirs (both sides
/// of a side by side row).
#[derive(Debug, Clone, Default)]
pub struct Sel {
    pub files: HashSet<usize>,
    pub hunks: HashSet<(usize, usize)>,
    pub lines: HashSet<(usize, usize, usize)>,
}

/// What rows select (diff::render): whole files from their name's row (f), whole hunks from their @@ row (h), lines
/// from theirs (lines, both sides of a side by side row).
pub fn picked<'a>(rows: impl IntoIterator<Item = &'a diff::Row>) -> Sel {
    let mut sel = Sel::default();
    for r in rows {
        sel.files.extend(r.f);
        sel.hunks.extend(r.h);
        sel.lines.extend(r.lines.iter().copied());
    }
    sel
}

fn is_change(l: &Line) -> bool {
    l.sign == '+' || l.sign == '-'
}

const NO_NL: &str = "\\ No newline at end of file";

/// The lines of a hunk (their indices) in the order the patch takes them. A run of changes kept in part pairs its
/// removed and added lines (the first removed with the first added, and so on), as git add -p does: a removed line
/// left out, turned context, then stays next to the line that replaced it instead of moving after all the added
/// ones. A "\ No newline at end of file" stays after its line.
fn order(hunk: &[Line], keep: &dyn Fn(usize) -> bool) -> Vec<usize> {
    let mut out = Vec::with_capacity(hunk.len());
    let mut i = 0;
    while i < hunk.len() {
        if !is_change(&hunk[i]) {
            out.push(i);
            i += 1;
            continue;
        }
        let start = i;
        let (mut rem, mut add): (Vec<Vec<usize>>, Vec<Vec<usize>>) = (Vec::new(), Vec::new());
        while i < hunk.len() && hunk[i].sign != ' ' {
            let mut item = vec![i];
            i += 1;
            if i < hunk.len() && hunk[i].sign == '\\' {
                item.push(i);
                i += 1;
            }
            if hunk[item[0]].sign == '-' { rem.push(item) } else { add.push(item) }
        }
        let run: Vec<usize> = (start..i).filter(|&k| is_change(&hunk[k])).collect();
        if rem.is_empty() || add.is_empty() || run.iter().all(|&k| keep(k)) || !run.iter().any(|&k| keep(k)) {
            out.extend(start..i);
            continue;
        }
        for k in 0..rem.len().max(add.len()) {
            out.extend(rem.get(k).into_iter().flatten());
            out.extend(add.get(k).into_iter().flatten());
        }
    }
    out
}

/// A file the mode acts on, with the changed lines selected in it ((hunk, line)); all when they are every change of
/// the file (or the file has none, a binary file or a mode change).
#[derive(Debug, Clone)]
pub struct Target {
    pub f: usize,
    pub keep: HashSet<(usize, usize)>,
    pub all: bool,
}

/// The files of the diff a mode acts on.
pub fn targets(parsed: &Diff, sel: &Sel, mode: Mode) -> Vec<Target> {
    let mut out = Vec::new();
    for (fi, f) in parsed.files.iter().enumerate() {
        if f.sec != mode.section() {
            continue;
        }
        let changes: Vec<(usize, usize)> = f
            .hunks
            .iter()
            .enumerate()
            .flat_map(|(hi, h)| h.lines.iter().enumerate().filter(|(_, l)| is_change(l)).map(move |(li, _)| (hi, li)))
            .collect();
        let keep: HashSet<(usize, usize)> = if sel.files.contains(&fi) {
            changes.iter().copied().collect()
        } else {
            changes
                .iter()
                .copied()
                .filter(|&(hi, li)| sel.hunks.contains(&(fi, hi)) || sel.lines.contains(&(fi, hi, li)))
                .collect()
        };
        if keep.is_empty() && !(sel.files.contains(&fi) && changes.is_empty()) {
            continue;
        }
        let all = keep.len() == changes.len();
        out.push(Target { f: fi, keep, all });
    }
    out
}

/// A path in a patch header, quoted as git does when it holds a quote, a backslash or a control character.
pub fn quote(p: &str) -> String {
    let special = |c: char| matches!(c, '"' | '\\' | '\0'..='\x1f' | '\x7f');
    if !p.chars().any(special) {
        return p.to_string();
    }
    let mut out = String::from("\"");
    for c in p.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if special(c) => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The name of a target's file: `to`, else `from`; "null" when neither is known.
fn name_of(f: &diff::File) -> String {
    let given = |s: &Option<String>| s.clone().filter(|s| !s.is_empty());
    given(&f.to).or_else(|| given(&f.from)).unwrap_or_else(|| "null".into())
}

fn sign_of(line: &str) -> char {
    line.chars().next().unwrap_or('\0')
}

/// The patch of a file for the lines kept, or None when none is left. Applied forward (stage), lines left out stay
/// as they are in the index: a removed line becomes context, an added one goes. Applied with -R (unstage, discard),
/// the patch must match what is there now, so the other way round: an added line left out becomes context, a removed
/// one goes. A whole added or deleted file keeps its headers; part of one is a change of the file (for an untracked
/// file, once git add -N has put it in the index, empty), but for part of a deletion taken back (-R), which brings
/// back the file with the lines kept. A whole file takes its mode change with it.
pub fn patch_of(parsed: &Diff, t: &Target, mode: Mode) -> Option<String> {
    let f = &parsed.files[t.f];
    let reverse = mode != Mode::Stage;
    let name = name_of(f);
    let (a, b) = (format!("a/{name}"), format!("b/{name}"));
    let mut head = vec![format!("diff --git {} {}", quote(&a), quote(&b))];
    let mode_of = || f.mode.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "100644".into());
    if t.all && f.status == "added" {
        head.extend([format!("new file mode {}", mode_of()), "--- /dev/null".into(), format!("+++ {}", quote(&b))]);
    } else if (t.all || reverse) && f.status == "deleted" {
        head.extend([format!("deleted file mode {}", mode_of()), format!("--- {}", quote(&a)), "+++ /dev/null".into()]);
    } else {
        if let (true, Some(old), Some(new)) = (t.all, &f.old_mode, &f.new_mode)
            && !old.is_empty()
            && !new.is_empty()
        {
            head.extend([format!("old mode {old}"), format!("new mode {new}")]);
        }
        head.extend([format!("--- {}", quote(&a)), format!("+++ {}", quote(&b))]);
    }
    let mut body: Vec<String> = Vec::new();
    let mut delta = 0i64; // lines added by the hunks before, on the side that changes
    for (hi, h) in f.hunks.iter().enumerate() {
        let keep = |li: usize| t.keep.contains(&(hi, li));
        let Some(m) = diff::head(&h.head) else { continue };
        let (o0, n0, rest) = (m.old, m.new, m.rest);
        if !(0..h.lines.len()).any(keep) {
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        let (mut shown, mut old_n, mut new_n) = (false, 0i64, 0i64);
        for li in order(&h.lines, &keep) {
            let l = &h.lines[li];
            // "\ No newline at end of file" follows the line it is about, when that line is in the patch.
            if l.sign == '\\' {
                if shown {
                    lines.push(NO_NL.into());
                }
                continue;
            }
            let sign = if l.sign == ' ' || keep(li) {
                Some(l.sign)
            } else if (l.sign == '-') != reverse {
                Some(' ')
            } else {
                None
            };
            shown = sign.is_some();
            let Some(sign) = sign else { continue };
            lines.push(format!("{sign}{}{}", l.text, if l.cr { "\r" } else { "" }));
            if sign != '+' {
                old_n += 1;
            }
            if sign != '-' {
                new_n += 1;
            }
        }
        // A removed or added line keeps its marker only while it ends its side: in reverse, an added line not chosen
        // turns context after the removed one, which then gets its newline back.
        let mut i = lines.len().saturating_sub(1);
        while i > 0 {
            let s = sign_of(&lines[i - 1]);
            if sign_of(&lines[i]) == '\\' && s != ' ' && s != '\\' {
                let later = lines[i + 1..].iter().any(|l| sign_of(l) == ' ' || sign_of(l) == s);
                if later {
                    lines.remove(i);
                }
            }
            i -= 1;
        }
        // A line without newline at the end of the file ends both sides when it is context: with lines kept after it
        // (an added line, the old last line turned context), it is the removed line, then the same line added with
        // its newline, as git would write it.
        if let Some(nl) = lines.iter().position(|l| l == NO_NL)
            && nl > 0
            && nl < lines.len() - 1
            && sign_of(&lines[nl - 1]) == ' '
        {
            let text = lines[nl - 1][1..].to_string();
            lines.splice(nl - 1..nl + 1, [format!("-{text}"), NO_NL.into(), format!("+{text}")]);
        }
        // The last lines of the file taken out leave the line before them last: without newline when the side made
        // had none (its last line, left out, had the marker), as the removed line, then the same line added without
        // it.
        let (gone, made) = if reverse { ('+', '-') } else { ('-', '+') };
        let n = lines.len();
        if n >= 2 && lines[n - 1] == NO_NL && sign_of(&lines[n - 2]) == gone {
            let mut j = n as i64 - 2;
            while j >= 0 && sign_of(&lines[j as usize]) == gone {
                j -= 1;
            }
            let unkept_end =
                h.lines.iter().enumerate().any(|(k, l)| {
                    l.sign == made && !keep(k) && h.lines.get(k + 1).is_some_and(|next| next.sign == '\\')
                });
            if j >= 0 && sign_of(&lines[j as usize]) == ' ' && unkept_end {
                let j = j as usize;
                let text = lines[j][1..].to_string();
                if reverse {
                    lines.splice(j..j + 1, [format!("-{text}"), NO_NL.into(), format!("+{text}")]);
                } else {
                    lines[j] = format!("-{text}");
                    lines.extend([format!("+{text}"), NO_NL.into()]);
                }
            }
        }
        // The side the patch is matched on keeps its place; the other moves with the lines kept before. A side with
        // no line starts at the line before (git's convention).
        let (oc, nc) = (m.old_count.unwrap_or(1), m.new_count.unwrap_or(1));
        let (mut old_start, mut new_start) = (o0, n0);
        if !reverse {
            let first = if oc != 0 { o0 } else { o0 + 1 } + delta;
            new_start = if new_n != 0 { first } else { first - 1 };
        } else {
            let first = if nc != 0 { n0 } else { n0 + 1 } - delta;
            old_start = if old_n != 0 { first } else { first - 1 };
        }
        delta += new_n - old_n;
        body.push(format!("@@ -{old_start},{old_n} +{new_start},{new_n} @@{rest}"));
        body.extend(lines);
    }
    if body.is_empty() {
        return None;
    }
    head.extend(body);
    head.push(String::new());
    Some(head.join("\n"))
}

/// Runs git in top, input on its standard input; code None when it could not start or took more than 2 minutes.
fn run_git(top: &str, env: &Env, args: &[&str], input: &str) -> Exec {
    let opts = Opts {
        cwd: Some(top.into()),
        env: Some(env.clone()),
        input: Some(input.as_bytes().to_vec()),
        timeout: Some(Duration::from_secs(120)),
        ..Opts::default()
    };
    exec(git(), args, &opts)
}

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n > 1 { "s" } else { "" })
}

/// What targets take: "2 files" (whole ones), "3 lines", "1 file" (without lines, a binary file).
fn summary(list: &[&Target]) -> String {
    let lines: usize = list.iter().map(|t| t.keep.len()).sum();
    if list.iter().all(|t| t.all) && list.len() > 1 {
        plural(list.len(), "file")
    } else if lines > 0 {
        plural(lines, "line")
    } else {
        plural(list.len(), "file")
    }
}

/// What a selection would take, for the question before discarding; "" for nothing.
pub fn count(parsed: &Diff, sel: &Sel, mode: Mode) -> String {
    let list = targets(parsed, sel, mode);
    if list.is_empty() { String::new() } else { summary(&list.iter().collect::<Vec<_>>()) }
}

/// Asks whether to delete files for good (their names), without a trash.
pub type Ask<'a> = &'a dyn Fn(&[String]) -> bool;

/// Does it: top, the repository's folder, which the diff's paths are relative to. Returns the message to show.
/// Untracked files discarded whole go to the trash (trash(path), false when there is none); without one, they are
/// deleted for good only when no_trash(names) says so, and kept otherwise.
pub fn apply(
    top: &str,
    parsed: &Diff,
    sel: &Sel,
    mode: Mode,
    trash: &dyn Fn(&str) -> bool,
    no_trash: Option<Ask>,
) -> String {
    let list = targets(parsed, sel, mode);
    if list.is_empty() {
        return if mode == Mode::Unstage { "Nothing staged selected" } else { "Nothing unstaged selected" }.into();
    }
    let mut env = git_env(top);
    env_set(&mut env, "GIT_LITERAL_PATHSPECS", "1");
    let why = |res: &Exec| {
        let text = if res.err.is_empty() { &res.out } else { &res.err };
        let first = text.trim_matches(crate::unicode::is_space).split('\n').next().unwrap_or("").to_string();
        if first.is_empty() { "git failed".to_string() } else { first }
    };
    let name = mode.name();
    let fail = |res: &Exec| format!("Cannot {name}: {}", why(res));
    let file = |t: &Target| &parsed.files[t.f];
    let untracked = |t: &Target| file(t).sec == "unstaged" && file(t).status == "added";
    let git_args = |fixed: &[&str], names: &[String]| -> Vec<String> {
        fixed.iter().map(|s| s.to_string()).chain(names.iter().cloned()).collect()
    };
    let git = |args: Vec<String>, input: &str| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_git(top, &env, &args, input)
    };
    // Added files the index already has (git add -N by the user): their entry is kept on a failure, and goes with
    // the file on a discard.
    let added: Vec<String> = list.iter().filter(|t| untracked(t)).map(|t| name_of(file(t))).collect();
    let indexed: HashSet<String> = if added.is_empty() {
        HashSet::new()
    } else {
        git(git_args(&["ls-files", "-z", "--"], &added), "").out.split('\0').map(String::from).collect()
    };
    let (mut patches, mut whole, mut ita, mut removed, mut unindex) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in &list {
        let name = name_of(file(t));
        // An untracked file: added whole with git add, removed whole from the disk; part of it goes through the index.
        let untracked = untracked(t);
        if file(t).hunks.is_empty() || (untracked && t.all) {
            if untracked && mode == Mode::Discard {
                if indexed.contains(&name) {
                    unindex.push(name.clone());
                }
                removed.push(name);
            } else {
                whole.push(name);
            }
        } else {
            if untracked && mode == Mode::Stage && !indexed.contains(&name) {
                ita.push(name);
            }
            patches.push(patch_of(parsed, t, mode));
        }
    }
    if !ita.is_empty() {
        let res = git(git_args(&["add", "-N", "--"], &ita), "");
        if res.code != Some(0) {
            return fail(&res);
        }
    }
    let patch: String = patches.into_iter().flatten().collect();
    let patched: Vec<&Target> = list
        .iter()
        .filter(|t| {
            let n = name_of(file(t));
            !whole.contains(&n) && !removed.contains(&n)
        })
        .collect();
    if !patch.is_empty() {
        let mut args = vec!["apply"];
        if mode != Mode::Discard {
            args.push("--cached");
        }
        if mode != Mode::Stage {
            args.push("-R");
        }
        args.extend(["--whitespace=nowarn", "-"]);
        let res = git(git_args(&args, &[]), &patch);
        // The files git add -N put in the index for it go back to untracked.
        if res.code != Some(0) {
            if !ita.is_empty() {
                git(git_args(&["rm", "--cached", "-q", "--"], &ita), "");
            }
            return fail(&res);
        }
    }
    if !whole.is_empty() {
        let has_head = git(git_args(&["rev-parse", "--verify", "-q", "HEAD"], &[]), "").code == Some(0);
        let args: &[&str] = match mode {
            Mode::Stage => &["add", "-A", "--"],
            Mode::Unstage if has_head => &["reset", "-q", "--"],
            Mode::Unstage => &["rm", "--cached", "-q", "--"],
            Mode::Discard => &["checkout", "-q", "--"],
        };
        let res = git(git_args(args, &whole), "");
        // The patch already applied stays: said, not undone.
        if res.code != Some(0) {
            if patch.is_empty() {
                return fail(&res);
            }
            let which = if whole.len() > 1 { plural(whole.len(), "file") } else { whole[0].clone() };
            return format!("{} {}; cannot {name} {which}: {}", mode.verb(), summary(&patched), why(&res));
        }
    }
    // The files deleted for good that could not be: the others are still deleted, and their index entry goes.
    let mut failed: Failed = Vec::new();
    // Listed by the diff, files all: one made a folder since is not taken with what it now holds.
    let mut left = remove_files(top, &removed, &mut failed, &|abs, _| Ok(trash(abs)), true);
    // Checked again by remove_files: a folder may have been made a link while the question was open.
    if !left.is_empty()
        && let Some(no_trash) = no_trash
        && no_trash(&left)
    {
        remove_files(top, &std::mem::take(&mut left), &mut failed, &for_good, true);
    }
    let is_failed = |n: &String| failed.iter().any(|(f, _)| f == n);
    let gone: Vec<String> = unindex.into_iter().filter(|n| !left.contains(n) && !is_failed(n)).collect();
    if !gone.is_empty() {
        let res = git(git_args(&["rm", "--cached", "-q", "--"], &gone), "");
        if res.code != Some(0) {
            return fail(&res);
        }
    }
    if left.is_empty() && failed.is_empty() {
        return format!("{} {}", mode.verb(), summary(&list.iter().collect::<Vec<_>>()));
    }
    // What went through, then what did not.
    let done: Vec<&Target> = list
        .iter()
        .filter(|t| {
            let n = name_of(file(t));
            !is_failed(&n) && !left.contains(&n)
        })
        .collect();
    let mut said = Vec::new();
    if !done.is_empty() {
        said.push(format!("{} {}", mode.verb(), summary(&done)));
    }
    if !left.is_empty() {
        said.push(format!("{} kept: no trash", plural(left.len(), "untracked file")));
    }
    if !failed.is_empty() {
        said.push(format!("cannot {}", cannot_delete(&failed)));
    }
    let said = said.join("; ");
    let mut chars = said.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The files that could not be deleted, in order: name → reason (set again, a name keeps its place).
pub type Failed = Vec<(String, String)>;

fn set_failed(failed: &mut Failed, name: &str, why: String) {
    match failed.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = why,
        None => failed.push((name.to_string(), why)),
    }
}

/// Deletes a file (abs, whether it is listed as a folder) for remove_files: Ok(true) once deleted, Ok(false) when it
/// could not (as the trash), Err(reason) when it failed.
pub type Del<'a> = &'a dyn Fn(&str, bool) -> Result<bool, String>;

/// Deletes the repository's files names (paths from its folder top, a folder listed as nested/) with del (for_good to
/// delete them for good): those refused or failing are set in failed (name → reason), those already gone are left
/// out, and those del did not take returned, but for folders, set in failed as never going for good (folderless). The
/// folders they leave empty go too. files: all of them listed as files, a folder among them refused.
pub fn remove_files(top: &str, names: &[String], failed: &mut Failed, del: Del, files: bool) -> Vec<String> {
    let mut left = Vec::new();
    for name in names {
        let (abs, folder) = located(top, name);
        let kind = if folder {
            Some(true)
        } else if files {
            Some(false)
        } else {
            None
        };
        match refused(&abs, top, kind).as_deref() {
            Some("gone") => continue,
            Some(why) => {
                set_failed(failed, name, why.to_string());
                continue;
            }
            None => {}
        }
        let done = match del(&abs, folder) {
            Ok(done) => done,
            Err(why) => {
                set_failed(failed, name, why);
                continue;
            }
        };
        // Gone meanwhile (the trash command then fails): nothing left to delete.
        if done || refused(&abs, top, kind).as_deref() == Some("gone") {
            prune_dirs(&abs, top);
        } else {
            left.push(name.clone());
        }
    }
    folderless(top, left, failed)
}

/// Deletes the file abs for good, for remove_files: a file, or a link itself (actions::unlink). Never a folder with
/// what it holds (an embedded repository and its history, or a file made a folder since it was listed): the trash
/// only; unlink fails on one made a folder after the check.
pub fn for_good(abs: &str, _folder: bool) -> Result<bool, String> {
    let st = match std::fs::symlink_metadata(abs) {
        Ok(st) => st,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(io_code(&e)),
    };
    if st.is_dir() {
        return Err(NO_TRASH_FOLDER.into());
    }
    match super::actions::unlink(abs, &st) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_code(&e)),
        _ => Ok(true),
    }
}

const NO_TRASH_FOLDER: &str = "a folder, never deleted for good";

/// The names (from remove_files' left) that may be asked about for good: the folders set in failed instead.
fn folderless(top: &str, names: Vec<String>, failed: &mut Failed) -> Vec<String> {
    names
        .into_iter()
        .filter(|name| {
            let dir = std::fs::symlink_metadata(located(top, name).0).is_ok_and(|m| m.is_dir());
            if dir {
                set_failed(failed, name, NO_TRASH_FOLDER.into());
            }
            !dir
        })
        .collect()
}

/// What failed (name → reason, from remove_files) says: delete <name or count>: <the first reason>.
pub fn cannot_delete(failed: &Failed) -> String {
    let (name, why) = &failed[0];
    let what = if failed.len() > 1 { plural(failed.len(), "file") } else { name.clone() };
    format!("delete {what}: {why}")
}

/// The folders left empty by the file abs gone, up to the repository's folder top (kept).
pub fn prune_dirs(abs: &str, top: &str) {
    let root = paths::resolve(&[top]);
    let inside = format!("{root}{}", paths::SEP);
    let mut d = paths::dirname(&paths::resolve(&[abs]));
    while d.starts_with(&inside) {
        if std::fs::remove_dir(&d).is_err() {
            break;
        }
        d = paths::dirname(&d);
    }
}

/// The repository's file name (a path from its folder top, a folder listed as nested/): its path without the final
/// separator, which would have lstat follow a folder made a link, and whether it is listed as a folder. A backslash
/// is a separator on Windows only: elsewhere, the last character of a file name.
pub fn located(top: &str, name: &str) -> (String, bool) {
    let is_sep = |c: char| c == '/' || (WIN && c == '\\');
    let abs = paths::join(&[top, name]);
    (abs.trim_end_matches(is_sep).to_string(), name.ends_with(is_sep))
}

/// Why the repository's file abs is not to be deleted as the file of that name: "gone" once it is no longer there
/// (its folder too, or made a file), else the reason to show (outside the repository, or reached through a symbolic
/// link: a folder made a link since the diff was read would lead to another file), or None when it can go. folder:
/// Some(true) when listed as a folder, which must still be one (not a link to one), Some(false) as a file, which must
/// not be one.
pub fn refused(abs: &str, top: &str, folder: Option<bool>) -> Option<String> {
    if !inside(abs, top) {
        return Some("outside the repository".into());
    }
    if let Some(link) = linked(abs, top) {
        return Some(link);
    }
    let st = match std::fs::symlink_metadata(abs) {
        Ok(st) => st,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some("gone".into()),
        Err(e) => {
            let code = io_code(&e);
            return Some(if code == "ENOTDIR" { "gone".into() } else { code });
        }
    };
    match folder {
        Some(true) if !st.is_dir() => Some("no longer a folder".into()),
        Some(false) if st.is_dir() => Some("no longer a file".into()),
        _ => None,
    }
}

/// Whether the file abs is in the repository's folder top, its folder too once symbolic links are followed (an index
/// naming sub/x while sub links to the home folder is not). A folder gone is judged by the nearest one left; one that
/// is there but cannot be followed (a link to nothing) is not in it.
fn inside(abs: &str, top: &str) -> bool {
    let real = |p: &str| walk_realpath(p).ok();
    let sep = paths::SEP;
    let Some(root) = real(top) else { return false };
    let resolved = paths::resolve(&[abs]);
    if !resolved.starts_with(&format!("{}{sep}", paths::resolve(&[top]))) {
        return false;
    }
    let mut d = paths::dirname(&resolved);
    loop {
        if let Some(dir) = real(&d) {
            return dir == root || dir.starts_with(&format!("{root}{sep}"));
        }
        // Up to the root of the drive without one to follow (top resolved, so only through a folder that cannot be
        // read).
        let up = paths::dirname(&d);
        if d == up {
            return false;
        }
        // A folder higher up made a file: this one cannot be there either.
        match std::fs::symlink_metadata(&d) {
            Ok(_) => return false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound || io_code(&e) == "ENOTDIR" => {}
            Err(_) => return false,
        }
        d = up;
    }
}

/// Why a folder between top and the file abs keeps it from being reached as is: "through a symbolic link" when one
/// is a link (the file itself may be one), the error when one cannot be read (EACCES), else None.
fn linked(abs: &str, top: &str) -> Option<String> {
    let mut d = paths::resolve(&[top]);
    let rel = paths::relative(&d, &paths::dirname(&paths::resolve(&[abs])));
    for part in rel.split(paths::SEP).filter(|p| !p.is_empty()) {
        d = paths::join(&[&d, part]);
        match std::fs::symlink_metadata(&d) {
            Ok(st) if st.file_type().is_symlink() => return Some("through a symbolic link".into()),
            Ok(_) => {}
            // A folder higher up made a file: nothing there to reach.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                let code = io_code(&e);
                return if code == "ENOTDIR" { None } else { Some(code) };
            }
        }
    }
    None
}
