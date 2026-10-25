//! scripts/fetch-bin.sh, without the network: a fake curl serves the files of a local folder laid out as GitHub's
//! release downloads and logs the URLs asked, a fake uname gives the platform, and PATH holds only those and the
//! tools the script needs.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

/// The tools fetch-bin and the fake curl run, linked from the system's folders; sha256sum or shasum (macOS) as found.
const TOOLS: [&str; 14] =
    ["sh", "sed", "head", "cut", "awk", "find", "rm", "mv", "chmod", "mkdir", "date", "sleep", "git", "cp"];
const HASHERS: [&str; 2] = ["sha256sum", "shasum"];

const CURL: &str = r#"#!/bin/sh
# curl -fsSL -o OUT URL: the file of $FAKE_RELEASES at the URL's path, 22 when missing.
out= url=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) ;;
    *) url=$1 ;;
  esac
  shift
done
echo "$url" >>"$FAKE_RELEASES/curl.log"
src="$FAKE_RELEASES/${url#https://github.com/}"
[ -f "$src" ] || exit 22
cp "$src" "$out"
"#;

const UNAME: &str = r#"#!/bin/sh
case "$1" in -s) echo "$FAKE_UNAME_S" ;; -m) echo "$FAKE_UNAME_M" ;; esac
"#;

struct Case {
    root: PathBuf,
    plugin: PathBuf,
    releases: PathBuf,
    path: String,
    uname: (String, String),
}

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn binary(version: &str) -> String {
    format!("#!/bin/sh\necho {version}\n")
}

fn system(tool: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].iter().map(|d| Path::new(d).join(tool)).find(|p| p.exists())
}

fn sha256(file: &Path) -> String {
    let out = match system("sha256sum") {
        Some(sum) => Command::new(sum).arg(file).output().unwrap(),
        None => Command::new("shasum").args(["-a", "256"]).arg(file).output().unwrap(),
    };
    String::from_utf8(out.stdout).unwrap().split(' ').next().unwrap().to_string()
}

impl Case {
    fn new(name: &str) -> Case {
        let root = std::env::temp_dir().join(format!("hx-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (plugin, releases, fake, tools) =
            (root.join("plugin"), root.join("releases"), root.join("fake"), root.join("tools"));
        for d in [&plugin, &releases, &fake, &tools] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(plugin.join("herdr-plugin.toml"), "id = \"gcaudan.explorer\"\nversion = \"9.9.9\"\n").unwrap();
        script(&fake.join("curl"), CURL);
        script(&fake.join("uname"), UNAME);
        for t in TOOLS {
            std::os::unix::fs::symlink(system(t).unwrap_or_else(|| panic!("{t} not found")), tools.join(t)).unwrap();
        }
        let hashers: Vec<_> = HASHERS.iter().filter_map(|t| Some((t, system(t)?))).collect();
        assert!(!hashers.is_empty(), "neither sha256sum nor shasum found");
        for (t, real) in hashers {
            std::os::unix::fs::symlink(real, tools.join(t)).unwrap();
        }
        let path = format!("{}:{}", fake.display(), tools.display());
        Case { root, plugin, releases, path, uname: ("Linux".into(), "x86_64".into()) }
    }

    /// A release of the repository: its assets (name, version printed) and a SHA256SUMS of them.
    fn release(&self, repo: &str, version: &str, assets: &[(&str, &str)]) {
        let dir = self.releases.join(repo).join("releases/download").join(format!("v{version}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sums = String::new();
        for (name, v) in assets {
            let f = dir.join(name);
            std::fs::write(&f, binary(v)).unwrap();
            sums += &format!("{}  {name}\n", sha256(&f));
        }
        std::fs::write(dir.join("SHA256SUMS"), sums).unwrap();
    }

    fn run(&self) -> Output {
        Command::new("/bin/sh")
            .arg(repo_root().join("scripts/fetch-bin.sh"))
            .arg(&self.plugin)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("FAKE_RELEASES", &self.releases)
            .env("FAKE_UNAME_S", &self.uname.0)
            .env("FAKE_UNAME_M", &self.uname.1)
            .output()
            .unwrap()
    }

    fn calls(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.releases.join("curl.log")).unwrap_or_default();
        log.lines().map(String::from).collect()
    }

    fn exe(&self) -> PathBuf {
        self.plugin.join("bin/herdr-explorer")
    }

    fn version(&self) -> String {
        let out = Command::new(self.exe()).arg("--version").output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// The files of bin/, sorted, the binaries set aside under one name.
    fn bin(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.plugin.join("bin"))
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        for n in &mut names {
            if n.starts_with("herdr-explorer.old-") {
                *n = "herdr-explorer.old-*".into();
            }
        }
        names.sort();
        names
    }

    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&self.plugin)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const LINUX: &str = "herdr-explorer-x86_64-unknown-linux-musl";
const URL: &str = "https://github.com/gcaudan/herdr-explorer/releases/download/v9.9.9";

#[test]
fn fetches_checks_and_places_the_binary_once() {
    let c = Case::new("once");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    let o = c.run();
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(c.version(), "9.9.9");
    assert_eq!(c.calls(), [format!("{URL}/{LINUX}"), format!("{URL}/SHA256SUMS")]);
    // no lock nor temporary file left
    assert_eq!(c.bin(), ["herdr-explorer"]);
    // That version there: nothing fetched again.
    assert!(c.run().status.success());
    assert_eq!(c.calls().len(), 2);
}

#[test]
fn replaces_an_older_binary_setting_it_aside() {
    let c = Case::new("older");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    std::fs::create_dir_all(c.plugin.join("bin")).unwrap();
    script(&c.exe(), &binary("1.0.0"));
    assert!(c.run().status.success());
    assert_eq!(c.version(), "9.9.9");
    assert_eq!(c.bin(), ["herdr-explorer", "herdr-explorer.old-*"]);
}

#[test]
fn a_development_binary_is_left_alone() {
    let c = Case::new("dev");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    std::fs::create_dir_all(c.plugin.join("bin")).unwrap();
    script(&c.exe(), &binary("0.0.1"));
    std::fs::write(c.plugin.join("bin/.dev"), "").unwrap();
    assert!(c.run().status.success());
    assert_eq!(c.version(), "0.0.1");
    assert!(c.calls().is_empty());
}

#[test]
fn a_wrong_hash_leaves_bin_as_it_was() {
    let c = Case::new("hash");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    let asset = c.releases.join("gcaudan/herdr-explorer/releases/download/v9.9.9").join(LINUX);
    std::fs::write(&asset, binary("6.6.6")).unwrap();
    std::fs::create_dir_all(c.plugin.join("bin")).unwrap();
    script(&c.exe(), &binary("1.0.0"));
    let o = c.run();
    assert!(!o.status.success());
    assert!(stderr(&o).contains("does not match its SHA256SUMS"), "{}", stderr(&o));
    assert_eq!(c.version(), "1.0.0");
    assert_eq!(c.bin(), ["herdr-explorer"]);
}

#[test]
fn a_missing_asset_or_sum_fails() {
    let c = Case::new("missing");
    let o = c.run();
    assert!(!o.status.success());
    assert!(stderr(&o).contains(&format!("cannot download {URL}/{LINUX}")), "{}", stderr(&o));
    c.release("gcaudan/herdr-explorer", "9.9.9", &[("herdr-explorer-aarch64-apple-darwin", "9.9.9")]);
    let dir = c.releases.join("gcaudan/herdr-explorer/releases/download/v9.9.9");
    std::fs::write(dir.join(LINUX), binary("9.9.9")).unwrap();
    let o = c.run();
    assert!(stderr(&o).contains(&format!("{LINUX} is not in SHA256SUMS")), "{}", stderr(&o));
    assert_eq!(c.bin(), Vec::<String>::new());
}

#[test]
fn each_platform_takes_its_asset() {
    for (s, m, asset) in [
        ("Linux", "aarch64", "herdr-explorer-aarch64-unknown-linux-musl"),
        ("Darwin", "arm64", "herdr-explorer-aarch64-apple-darwin"),
        ("Darwin", "x86_64", "herdr-explorer-x86_64-apple-darwin"),
    ] {
        let mut c = Case::new(&format!("{s}-{m}"));
        c.uname = (s.into(), m.into());
        c.release("gcaudan/herdr-explorer", "9.9.9", &[(asset, "9.9.9")]);
        let o = c.run();
        assert!(o.status.success(), "{s} {m}: {}", stderr(&o));
        assert_eq!(c.calls()[0], format!("{URL}/{asset}"));
    }
}

#[test]
fn a_platform_without_an_asset_says_so() {
    let mut c = Case::new("armv7");
    c.uname = ("Linux".into(), "armv7l".into());
    let o = c.run();
    assert!(!o.status.success());
    assert!(stderr(&o).contains("no release binary for Linux armv7l"), "{}", stderr(&o));
    assert!(c.calls().is_empty());
}

#[test]
fn a_github_origin_gives_the_repository() {
    for (origin, repo) in [
        ("git@github.com:me/fork.git", "me/fork"),
        ("https://github.com/me/fork", "me/fork"),
        ("ssh://git@github.com/me/fork.git/", "me/fork"),
        ("https://gitlab.com/me/fork.git", "gcaudan/herdr-explorer"),
        ("https://github.com/../fork", "gcaudan/herdr-explorer"),
    ] {
        let c = Case::new("origin");
        c.git(&["init", "-q"]);
        c.git(&["remote", "add", "origin", origin]);
        c.release(repo, "9.9.9", &[(LINUX, "9.9.9")]);
        let o = c.run();
        assert!(o.status.success(), "{origin}: {}", stderr(&o));
        assert_eq!(c.calls()[0], format!("https://github.com/{repo}/releases/download/v9.9.9/{LINUX}"), "{origin}");
    }
}

#[test]
fn without_curl_nothing_is_touched() {
    let mut c = Case::new("nocurl");
    std::fs::remove_file(c.root.join("fake/curl")).unwrap();
    c.path = format!("{}:{}", c.root.join("fake").display(), c.root.join("tools").display());
    let o = c.run();
    assert!(!o.status.success());
    assert!(stderr(&o).contains("curl not found"), "{}", stderr(&o));
    assert!(!c.plugin.join("bin").exists());
}

#[test]
fn waits_for_a_fetch_under_way_then_finds_its_binary() {
    let c = Case::new("lock");
    let bin = c.plugin.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(".fetch.lock"), "").unwrap();
    let (exe, lock) = (c.exe(), bin.join(".fetch.lock"));
    let other = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        script(&exe, &binary("9.9.9"));
        std::fs::remove_file(lock).unwrap();
    });
    let o = c.run();
    other.join().unwrap();
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(c.calls().is_empty());
    assert_eq!(c.bin(), ["herdr-explorer"]);
}

#[test]
fn a_lock_left_by_a_dead_fetch_is_taken() {
    let c = Case::new("stale");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    let lock = c.plugin.join("bin/.fetch.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let f = std::fs::File::create(&lock).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(300)).unwrap();
    let o = c.run();
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(c.version(), "9.9.9");
    assert!(!lock.exists());
}

#[test]
fn a_bin_folder_not_writable_fails_instead_of_waiting() {
    let c = Case::new("readonly");
    c.release("gcaudan/herdr-explorer", "9.9.9", &[(LINUX, "9.9.9")]);
    let bin = c.plugin.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).unwrap();
    // root writes anyway: nothing to check then.
    let writable = std::fs::write(bin.join("probe"), "").is_ok();
    let o = c.run();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return;
    }
    assert!(!o.status.success());
    assert!(stderr(&o).contains("cannot create"), "{}", stderr(&o));
    assert!(c.calls().is_empty());
}
