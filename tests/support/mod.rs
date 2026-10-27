//! What the integration tests share: temporary folders, and the replay of the module fixtures.

#![allow(dead_code)]

#[cfg(target_os = "linux")]
pub mod pty;
pub mod vt;

use herdr_explorer::json::{self, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Held by every test that sets environment variables: the tests of a binary run in parallel threads.
pub static ENV: Mutex<()> = Mutex::new(());

/// A fresh folder, removed when dropped; `path` has its symbolic links resolved.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        let path = herdr_explorer_canonical(&dir);
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn herdr_explorer_canonical(p: &Path) -> PathBuf {
    let real = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    // canonicalize gives \\?\C:\… on Windows, which the fixtures never hold.
    match real.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(s) if !s.starts_with("UNC") => PathBuf::from(s),
        _ => real,
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/modules")
}

/// The fixture file of a module, such as "core/places".
pub fn load(module: &str) -> Value {
    let file = fixtures_dir().join(format!("{}.json", module.replace('/', ".")));
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
}

/// Every string of v, keys included, with `from` replaced by `to` (as a replace in the whole JSON text).
pub fn replace(v: &Value, from: &str, to: &str) -> Value {
    match v {
        Value::Str(s) => Value::Str(s.replace(from, to)),
        Value::Arr(a) => Value::Arr(a.iter().map(|x| replace(x, from, to)).collect()),
        Value::Obj(o) => {
            let mut fields = Vec::new();
            for (k, x) in o {
                json::insert(&mut fields, k.replace(from, to), replace(x, from, to));
            }
            Value::Obj(fields)
        }
        other => other.clone(),
    }
}

/// The environment as it was, put back when dropped, even when the call panicked.
struct EnvBack(Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>);

impl EnvBack {
    /// Removes herdr's and the explorer's variables (those of a real session), then sets `set`.
    fn new(set: &[(String, String)]) -> EnvBack {
        let mut saved: Vec<_> = std::env::vars_os()
            .filter(|(k, _)| k.to_str().is_some_and(|k| k.starts_with("HERDR_") || k.starts_with("EXPLORER_")))
            .map(|(k, v)| (k, Some(v)))
            .collect();
        saved.extend(set.iter().map(|(k, _)| (k.into(), std::env::var_os(k))));
        for (k, _) in &saved {
            // SAFETY: tests that touch the environment hold ENV.
            unsafe { std::env::remove_var(k) };
        }
        for (k, v) in set {
            // SAFETY: as above.
            unsafe { std::env::set_var(k, v) };
        }
        EnvBack(saved)
    }
}

impl Drop for EnvBack {
    fn drop(&mut self) {
        for (k, v) in self.0.iter().rev() {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

/// Makes a case's files under root: text, a folder (null) or a symbolic link ({ link }).
fn setup(root: &Path, files: Option<&Value>) {
    let Some(files) = files.and_then(Value::as_object) else {
        return;
    };
    for (name, content) in files {
        let p = root.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        match content {
            Value::Null => std::fs::create_dir_all(&p).unwrap(),
            Value::Str(text) => std::fs::write(&p, text.replace("$ROOT", root.to_str().unwrap())).unwrap(),
            other => {
                let target = other.get("link").and_then(Value::as_str).unwrap();
                let target = target.replace("$ROOT", root.to_str().unwrap());
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &p).unwrap();
                #[cfg(not(unix))]
                panic!("symbolic link {target}: fixtures replay on Unix only");
            }
        }
    }
}

/// Replays every case (Unix only: the fixtures were recorded on Linux, with its paths; Windows is covered by the
/// unit tests) of a module's fixture: `call(fn, args)` gives what the Rust returns, `Err` for an error. Panics with
/// the list of the cases that differ; with `HX_RECORD=1`, writes what the Rust returns as the
/// outcome of those that differ only by it, for a change of behaviour its commit explains.
pub fn replay(module: &str, call: impl Fn(&str, &[Value]) -> Result<Value, String>) {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let mut fixture = load(module);
    let record = std::env::var_os("HX_RECORD").is_some();
    let mut recorded = Vec::new();
    let mut failures = Vec::new();
    for (i, case) in fixture.get("cases").and_then(Value::as_array).unwrap().iter().enumerate() {
        let dir = TempDir::new("fixture");
        let root = dir.path.to_str().unwrap().to_string();
        setup(&dir.path, case.get("files"));
        let env: Vec<(String, String)> = case
            .get("env")
            .and_then(Value::as_object)
            .map(|e| e.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").replace("$ROOT", &root))).collect())
            .unwrap_or_default();
        let back = EnvBack::new(&env);
        let name = case.get("fn").and_then(Value::as_str).unwrap();
        let args = replace(case.get("args").unwrap(), "$ROOT", &root);
        let got = call(name, args.as_array().unwrap());
        drop(back);
        let outcome = got.clone().map(|v| replace(&v, &root, "$ROOT"));
        let got = match got {
            Ok(v) => json::stringify(&replace(&v, &root, "$ROOT")),
            Err(e) => format!("throws {e:?}"),
        };
        let want = match case.get("throws") {
            Some(t) => format!("throws {:?}", t.as_str().unwrap_or("")),
            None => json::stringify(case.get("out").unwrap_or(&Value::Null)),
        };
        let mut diff = Vec::new();
        if got != want {
            // Recorded when the outcome keeps its kind: a result for a result, an error for an error.
            match outcome {
                o if record && o.is_err() == case.get("throws").is_some() => recorded.push((i, o)),
                _ => diff.push(format!("got {got}, want {want}")),
            }
        }
        if let Some(after) = case.get("after").and_then(Value::as_object) {
            for (file, want) in after {
                let got = std::fs::read_to_string(dir.path.join(file)).ok().map(|t| t.replace(&root, "$ROOT"));
                if Value::from(got.clone()) != *want {
                    diff.push(format!("{file}: got {got:?}, want {}", json::stringify(want)));
                }
            }
        }
        if !diff.is_empty() {
            failures.push(format!("#{i} {name}({}): {}", json::stringify(case.get("args").unwrap()), diff.join("; ")));
        }
    }
    if !recorded.is_empty() {
        if let Value::Obj(fields) = &mut fixture
            && let Some((_, Value::Arr(cases))) = fields.iter_mut().find(|(k, _)| k == "cases")
        {
            for (i, outcome) in recorded {
                match outcome {
                    Ok(out) => cases[i].set("out", out),
                    Err(e) => cases[i].set("throws", Value::Str(e)),
                }
            }
        }
        let file = fixtures_dir().join(format!("{}.json", module.replace('/', ".")));
        std::fs::write(file, format!("{}\n", json::stringify_pretty(&fixture, 1))).unwrap();
    }
    assert!(failures.is_empty(), "{module}: {} case(s) differ\n{}", failures.len(), failures.join("\n"));
}
