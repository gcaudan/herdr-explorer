//! test/explorer-tree.test.js: the explorer's tree without the terminal: the rows of a folder with its expanded
//! folders, their order, the filter, the changed files only, and the git decorations read from a status.

mod support;

use herdr_explorer::explorer::tree::{self, Entry, Settings, Tree};
use std::path::Path;

/// A folder made of paths: a trailing / for a folder, anything else a file.
fn folder(paths: &[&str]) -> support::TempDir {
    let dir = support::TempDir::new("explorer-tree");
    for p in paths {
        let full = dir.path.join(p);
        if p.ends_with('/') {
            std::fs::create_dir_all(&full).unwrap();
        } else {
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, "").unwrap();
        }
    }
    dir
}

fn root(dir: &support::TempDir) -> String {
    dir.path.to_string_lossy().into_owned()
}

fn mark(r: &Entry) -> &'static str {
    match (r.dir, r.open) {
        (false, _) => "",
        (true, true) => "/-",
        (true, false) => "/+",
    }
}

fn shown(rows: &[Entry]) -> Vec<String> {
    rows.iter().map(|r| format!("{}{}{}", "  ".repeat(r.depth), r.name, mark(r))).collect()
}

fn chained(rows: &[Entry]) -> Vec<String> {
    rows.iter().map(|r| format!("{}{}{}", r.chain.as_deref().unwrap_or(""), r.name, mark(r))).collect()
}

/// A status line of git status --porcelain=v1 -z.
fn status(lines: &[&str]) -> String {
    lines.iter().map(|l| format!("{l}\0")).collect()
}

fn set(rels: &[&str]) -> Vec<String> {
    rels.iter().map(|r| r.to_string()).collect()
}

fn settings(hidden: bool, ignored: bool, compact: bool) -> Settings {
    Settings { hidden, ignored, compact }
}

#[test]
fn rows_folders_first_names_in_numeric_order_expanded_folders_unfolded() {
    let dir = folder(&[
        "b.txt",
        "a10.txt",
        "a9.txt",
        "Zeta/",
        "alpha/",
        "alpha/inner/",
        "alpha/inner/deep.md",
        "alpha/x.js",
        ".hidden",
        ".git/HEAD",
    ]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    assert_eq!(shown(&tree.rows(None, false, &set(&[""]))), ["alpha/+", "Zeta/+", "a9.txt", "a10.txt", "b.txt"]);
    assert_eq!(
        shown(&tree.rows(None, false, &set(&["", "alpha", "alpha/inner"]))),
        ["alpha/-", "  inner/-", "    deep.md", "  x.js", "Zeta/+", "a9.txt", "a10.txt", "b.txt"]
    );
    // An expanded folder inside a collapsed one stays hidden.
    assert_eq!(shown(&tree.rows(None, false, &set(&["alpha/inner"])))[..2], ["alpha/+", "Zeta/+"]);
    assert_eq!(tree.rows(None, false, &[])[1].rel, "Zeta");
}

#[test]
fn rows_dot_files_with_the_hidden_setting_never_git_the_cache_until_cleared() {
    let dir = folder(&[".hidden", ".git/HEAD", "a.txt"]);
    let mut tree = Tree::new(&root(&dir), settings(true, true, false));
    assert_eq!(shown(&tree.rows(None, false, &[])), [".hidden", "a.txt"]);
    tree.settings.hidden = false;
    assert_eq!(shown(&tree.rows(None, false, &[])), ["a.txt"]);
    std::fs::write(dir.path.join("b.txt"), "").unwrap();
    assert_eq!(shown(&tree.rows(None, false, &[])), ["a.txt"], "read from the cache");
    tree.cache.clear();
    assert_eq!(shown(&tree.rows(None, false, &[])), ["a.txt", "b.txt"]);
}

#[test]
fn rows_a_folded_compact_folder_is_chained_only_through_folders_already_read() {
    let dir = folder(&["a/b/c/x.txt", "z.txt"]);
    let mut tree = Tree::new(&root(&dir), settings(true, true, true));
    assert_eq!(chained(&tree.rows(None, false, &set(&[""]))), ["a/+", "z.txt"]);
    assert!(!tree.cache.contains_key("a"), "a folded folder is not read");
    // Unfolded from its row (a): the whole chain, open.
    assert_eq!(chained(&tree.rows(None, false, &set(&["", "a"]))), ["a/b/c/-", "x.txt", "z.txt"]);
    // Folded again, the chain read stays.
    assert_eq!(chained(&tree.rows(None, false, &set(&[""]))), ["a/b/c/+", "z.txt"]);
}

fn age(p: &Path, secs: u64) {
    set_time(p, std::time::SystemTime::now() - std::time::Duration::from_secs(secs));
}

/// A folder's modification time; Windows opens a folder only with backup semantics.
fn set_time(p: &Path, time: std::time::SystemTime) {
    #[cfg(windows)]
    let f = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::File::options().write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(p).unwrap()
    };
    #[cfg(not(windows))]
    let f = std::fs::File::open(p).unwrap();
    f.set_modified(time).unwrap();
}

#[test]
fn stale_a_folder_is_read_again_only_when_its_modification_time_moved() {
    let dir = folder(&["a.txt"]);
    age(&dir.path, 60);
    let mut tree = Tree::new(&root(&dir), settings(true, true, false));
    let first = tree.read_dir("");
    tree.stale();
    assert_eq!(tree.read_dir(""), first, "same time: reused");
    // A file added behind the folder's back, its time put back: still the list read.
    let time = std::fs::metadata(&dir.path).unwrap().modified().unwrap();
    std::fs::write(dir.path.join("hidden.txt"), "").unwrap();
    set_time(&dir.path, time);
    tree.stale();
    assert_eq!(tree.read_dir(""), first, "same time: not read again");
    std::fs::write(dir.path.join("b.txt"), "").unwrap();
    tree.stale();
    assert_eq!(tree.read_dir("").iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.txt", "b.txt", "hidden.txt"]);
}

#[test]
fn rows_the_filter_keeps_the_matches_and_the_folders_holding_one_unfolded_outside_ignored_folders() {
    let dir = folder(&[
        "src/",
        "src/main.js",
        "src/util.js",
        "docs/",
        "docs/Main.md",
        "node_modules/",
        "node_modules/main.js",
        "readme.md",
    ]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    tree.git.top = Some(root(&dir));
    tree.apply_status(&status(&["!! node_modules/"]));
    let rows = tree.rows(Some("main"), false, &[]);
    // node_modules/main.js is not read: the ignored folder shows only when its own name matches.
    assert_eq!(shown(&rows), ["docs/-", "  Main.md", "src/-", "  main.js"]);
    assert_eq!(rows.iter().map(|r| r.hit).collect::<Vec<_>>(), [None, Some((0, 4)), None, Some((0, 4))]);
    // The folder's own name matches: shown folded, its contents unread.
    assert_eq!(shown(&tree.rows(Some("node"), false, &[])), ["node_modules/+"]);
    assert_eq!(tree.rows(Some("nothing"), false, &[]), []);
}

#[test]
fn rows_the_filter_marks_the_match_in_the_name_as_shown() {
    // Not in the JS test: the match found in the name in lower case, marked in the name itself.
    let dir = folder(&["éTÉ.txt", "x/Été.md"]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    let rows = tree.rows(Some("té"), false, &[]);
    assert_eq!(shown(&rows), ["x/-", "  Été.md", "éTÉ.txt"]);
    let marked: Vec<_> = rows.iter().filter_map(|r| r.hit.map(|(a, b)| &r.name[a..b])).collect();
    assert_eq!(marked, ["té", "TÉ"]);
}

#[test]
fn git_status_codes_folders_with_their_strongest_change_renames_prefix() {
    let codes: Vec<char> = ["??", "!!", "UU", "AA", "DD", " D", "R ", "A ", " M", "MM"]
        .iter()
        .map(|x| {
            let c: Vec<char> = x.chars().collect();
            tree::code(c[0], c[1])
        })
        .collect();
    assert_eq!(codes, ['U', '!', 'C', 'C', 'C', 'D', 'R', 'A', 'M', 'M']);
    let git = tree::parse_status(
        &status(&[
            " M sub/a/x.js",
            "D  sub/a/y.js",
            "R  sub/new.js",
            "sub/old.js",
            "?? sub/tmp/",
            "!! sub/dist/",
            " M other/z.js",
        ]),
        "sub",
    );
    assert_eq!(
        git.files.iter().map(|(k, &v)| (k, v)).collect::<Vec<_>>(),
        [("a/x.js", 'M'), ("a/y.js", 'D'), ("new.js", 'R')]
    );
    assert_eq!(git.dirs.iter().map(|(k, &v)| (k, v)).collect::<Vec<_>>(), [("a", 'D'), ("", 'D')]);
    assert_eq!(git.untracked.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["tmp"]);
    assert_eq!(git.ignored.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["dist"]);
}

#[test]
fn decorations_a_file_a_folder_and_the_contents_of_untracked_and_ignored_folders() {
    let dir = folder(&["a/", "a/x.js", "a/y.js", "new/", "new/n.js", "build/", "build/out.js", "clean.js"]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    tree.apply_status(&status(&[" M a/x.js", "A  a/y.js", "?? new/", "!! build/", " D gone/old.js"]));
    let deco: Vec<Option<char>> = ["a", "a/x.js", "a/y.js", "new", "new/n.js", "build/out.js", "clean.js"]
        .iter()
        .map(|rel| tree.decoration(rel, dir.path.join(rel).is_dir()))
        .collect();
    assert_eq!(deco, [Some('M'), Some('M'), Some('A'), Some('U'), Some('U'), Some('!'), None]);
    assert_eq!(tree.git.gone, ["gone/old.js"]);
    // A change inside a collapsed ignored folder is not visible.
    assert!(tree.hidden("build/out.js", &[]));
    assert!(!tree.hidden("build/out.js", &set(&["build"])));
    assert!(!tree.hidden("a/x.js", &[]));
    // Ignored entries left out unless shown.
    assert_eq!(shown(&tree.rows(None, false, &[])), ["a/+", "build/+", "new/+", "clean.js"]);
    let mut hiding = Tree::new(&root(&dir), settings(false, false, false));
    hiding.apply_status(&status(&["!! build/"]));
    assert_eq!(shown(&hiding.rows(None, false, &[])), ["a/+", "new/+", "clean.js"]);
}

#[test]
fn repositories_of_the_root_folders_their_statuses_merged_each_entry_with_its_repository() {
    let dir = folder(&["one/a.js", "one/b.js", "two/c.js", "two/dist/out.js", "loose.txt"]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    assert!(!tree.git.on());
    tree.git.repos = set(&["one", "two"]);
    assert!(tree.git.on());
    tree.apply_statuses(&[("one", &status(&[" M a.js", " D gone.js"])), ("two", &status(&["?? c.js", "!! dist/"]))]);
    let deco: Vec<Option<char>> = ["one", "one/a.js", "one/b.js", "two", "two/c.js", "two/dist/out.js", "loose.txt"]
        .iter()
        .map(|rel| tree.decoration(rel, dir.path.join(rel).is_dir()))
        .collect();
    assert_eq!(deco, [Some('D'), Some('M'), None, Some('U'), Some('U'), Some('!'), None]);
    assert_eq!(tree.git.dirs.get(""), Some(&'D'));
    assert_eq!(tree.git.gone, ["one/gone.js"]);
    assert_eq!(tree.changed_files(), ["one/a.js", "one/gone.js", "two/c.js"]);
    assert_eq!(
        ["one/a.js", "two", "loose.txt", ""].map(|r| tree.git.repo_of(r)),
        [Some("one"), Some("two"), None, None]
    );
    // The root in a repository: its own for every entry.
    tree.git.top = Some(root(&dir));
    assert_eq!(tree.git.repo_of("loose.txt"), Some(""));
}

#[test]
fn changed_files_only_every_folder_unfolded_deleted_files_among_them_in_the_tree_order() {
    let dir = folder(&["a/", "a/x.js", "a/same.js", "b.js", "c.js"]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, false));
    tree.apply_status(&status(&[" M a/x.js", " D a/gone.js", " D old/f.js", "?? c.js"]));
    let rows = tree.rows(None, true, &[]);
    assert_eq!(shown(&rows), ["a/-", "  gone.js", "  x.js", "old/-", "  f.js", "c.js"]);
    assert_eq!(
        rows.iter().filter(|r| r.gone).map(|r| r.rel.as_str()).collect::<Vec<_>>(),
        ["a/gone.js", "old", "old/f.js"]
    );
    assert_eq!(tree.changed_files(), ["a/gone.js", "a/x.js", "old/f.js", "c.js"]);
    assert_eq!(shown(&tree.rows(Some("x"), true, &[])), ["a/-", "  x.js"]);
}

#[test]
fn tree_order_folders_before_files_at_each_level_names_in_numeric_order() {
    assert!(tree::tree_before("a/z.js", false, "b.js", false));
    assert!(!tree::tree_before("b.js", false, "a/z.js", false));
    assert!(tree::tree_before("f2.js", false, "f10.js", false));
    assert!(tree::tree_before("a", true, "a/x.js", false));
    assert!(!tree::tree_before("a/x.js", false, "a", true));
    let entry = |name: &str, dir: bool| Entry { name: name.into(), dir, ..Entry::default() };
    let mut entries = [entry("b", false), entry("B2", true), entry("a10", false), entry("a9", false)];
    entries.sort_by(tree::entry_order);
    assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["B2", "a9", "a10", "b"]);
    assert_eq!(["", "a", "a/b", "a/b/c"].map(tree::parent_of), [None, Some(""), Some("a"), Some("a/b")]);
}

#[test]
fn compact_folders_a_folder_holding_only_a_folder_shares_its_row_as_the_last_one() {
    let dir = folder(&[
        "src/main/java/App.java",
        "src/main/java/Util.java",
        "lib/a.js",
        "lib/sub/b.js",
        "only/one.txt",
        "readme.md",
    ]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, true));
    let rows = tree.rows(None, false, &set(&["", "src/main/java"]));
    assert_eq!(
        rows.iter()
            .map(|r| format!("{}{}{}{}", "  ".repeat(r.depth), r.chain.as_deref().unwrap_or(""), r.name, mark(r)))
            .collect::<Vec<_>>(),
        ["lib/+", "only/+", "src/main/java/-", "  App.java", "  Util.java", "readme.md"]
    );
    let java = &rows[2];
    assert_eq!(
        (java.rel.as_str(), java.from.as_deref(), java.chain.as_deref()),
        ("src/main/java", Some("src"), Some("src/main/"))
    );
    // The row stands for every folder of its chain, no other entry.
    assert!(["src", "src/main", "src/main/java"].iter().all(|rel| tree::holds(java, rel)));
    assert!(!tree::holds(java, "src/main/java/App.java") && !tree::holds(&rows[0], "lib/sub"));
    // Not while filtering, nor with the setting off.
    assert_eq!(
        tree.rows(Some("app"), false, &[]).iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["src", "main", "java", "App.java"]
    );
    tree.settings.compact = false;
    assert_eq!(
        tree.rows(None, false, &set(&["src"])).iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["lib", "only", "src", "main", "readme.md"]
    );
}

#[test]
fn compact_folders_among_the_changed_files_the_changed_entries_only_count() {
    let dir = folder(&["a/b/c.js", "a/keep.js", "z.js"]);
    let mut tree = Tree::new(&root(&dir), settings(false, true, true));
    tree.apply_status(&status(&[" M a/b/c.js"]));
    assert_eq!(
        tree.rows(None, true, &[])
            .iter()
            .map(|r| format!("{}:{}{}", r.depth, r.chain.as_deref().unwrap_or(""), r.name))
            .collect::<Vec<_>>(),
        ["0:a/b", "1:c.js"]
    );
}
