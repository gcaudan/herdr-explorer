//! A mutation bench for the decoders and parsers: the inputs of the module fixtures, altered at random (bits flipped,
//! bytes cut, repeated, inserted or spliced from another input), go through each of them. A panic, or a call longer
//! than 2 s, fails the test and leaves the input in target/mutate/. `MUTATE_ROUNDS` sets the calls per target (200 by
//! default, more for the nightly pass of CI); `MUTATE_SEED` and the same rounds replay a run.

use herdr_explorer::codec::{base64, inflate};
use herdr_explorer::core::regex::Regex;
use herdr_explorer::json::{self, Value};
use herdr_explorer::render::{highlight, image, markdown};
use herdr_explorer::{diff, scm};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// xorshift64*: enough to scatter mutations, and the same run for the same seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(rng: &mut Rng, seed: &[u8], other: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(b.len() + 1);
        match rng.below(6) {
            0 if !b.is_empty() => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 => b.truncate(at),
            2 => {
                let end = (at + 1 + rng.below(64)).min(b.len());
                let part = b[at.min(end)..end].to_vec();
                b.splice(at..at, part.iter().copied().cycle().take(part.len() * (1 + rng.below(8))));
            }
            3 => b.insert(at, rng.next() as u8),
            4 => {
                let end = (at + rng.below(64)).min(b.len());
                b.drain(at..end);
            }
            _ => {
                let from = rng.below(other.len() + 1);
                let end = (from + rng.below(256)).min(other.len());
                b.splice(at..at, other[from..end].iter().copied());
            }
        }
    }
    b
}

/// Every string among the arguments of the cases of a fixture, or of those calling one of `fns` when given.
fn strings(fixture: &str, fns: &[&str]) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Str(s) => out.push(s.clone()),
            Value::Arr(items) => items.iter().for_each(|x| walk(x, out)),
            Value::Obj(fields) => fields.iter().for_each(|(_, x)| walk(x, out)),
            _ => {}
        }
    }
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/modules").join(fixture);
    let v = json::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut out = Vec::new();
    for case in v.get("cases").and_then(Value::as_array).unwrap() {
        let name = case.get("fn").and_then(Value::as_str).unwrap_or("");
        if fns.is_empty() || fns.contains(&name) {
            walk(case.get("args").unwrap_or(&Value::Null), &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

type Target = (&'static str, fn(&[u8]));

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const BYTES: &[Target] = &[
    ("image::info", |b| drop(image::info(b))),
    ("image::decode", |b| drop(image::decode(b))),
    ("image::pixels", |b| drop(image::pixels(b, 40.0, 20.0))),
    ("image::render", |b| drop(image::render(b, 20.0, 10.0, None))),
    ("inflate_zlib", |b| drop(inflate::inflate_zlib(b, 1 << 20))),
];

const TEXTS: &[Target] = &[
    ("json::parse", |b| drop(json::parse(&text(b)))),
    ("base64::decode", |b| drop(base64::decode(&text(b)))),
    ("diff::parse", |b| {
        let d = diff::parse(&text(b));
        diff::widest_of(&d);
        drop(diff::render(&d, 60, "unified", true, &Default::default()));
        drop(diff::render(&d, 120, "split", false, &Default::default()));
    }),
    ("scm::parse", |b| {
        let t = text(b);
        drop(scm::parse::parse_status(&t));
        drop(scm::parse::parse_history(&t));
        drop(scm::parse::parse_stashes(&t));
        drop(scm::parse::parse_refs(&t, &t, &t));
        drop(scm::parse::parse_names(&t));
        drop(scm::parse::parse_numstat(&t));
        drop(scm::parse::parse_scopes(&t));
        drop(scm::parse::parse_repo(&t, "/r"));
        drop(scm::parse::ref_list(&t));
    }),
    ("markdown::render", |b| drop(markdown::render(&text(b), 60.0, markdown::Options::default()))),
    ("highlight", |b| {
        let t = text(b);
        for lang in ["rust", "javascript", "python", "shell", "html", "markdown", "json", "sql", "yaml", "css"] {
            drop(highlight::highlight(&t, Some(lang)));
        }
    }),
    ("regex", |b| {
        let t = text(b);
        if let Ok(re) = Regex::new(&t, "i") {
            drop(re.exec(&t));
        }
    }),
];

/// Runs f on input in a thread of its own: Err with what went wrong on a panic or past the time limit.
fn call(f: fn(&[u8]), input: &[u8]) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let owned = input.to_vec();
    let worker = std::thread::spawn(move || {
        f(&owned);
        let _ = tx.send(());
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(()) => Ok(()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err("longer than 2 s".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(worker.join().err().and_then(|e| e.downcast_ref::<String>().cloned()).unwrap_or("panic".into()))
        }
    }
}

#[test]
fn decoders_and_parsers_survive_mutated_inputs() {
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok());
    let rounds = env("MUTATE_ROUNDS").unwrap_or(200) as usize;
    let seed = env("MUTATE_SEED").unwrap_or(0x9e37_79b9_7f4a_7c15);
    let mut rng = Rng(seed | 1);
    let images: Vec<Vec<u8>> =
        strings("render.image.json", &["info", "decode"]).iter().map(|s| base64::decode(s)).collect();
    let mut zlib: Vec<Vec<u8>> = strings("codec.json", &["inflate"]).iter().map(|s| base64::decode(s)).collect();
    zlib.push(herdr_explorer::codec::deflate::deflate_zlib(include_bytes!("mutate.rs")));
    let texts: Vec<Vec<u8>> = ["scm.parse.json", "diff.json", "render.markdown.json", "render.highlight.json"]
        .iter()
        .flat_map(|f| strings(f, &[]))
        .chain(strings("regexp.json", &["exec"]).into_iter().take(200))
        .map(String::into_bytes)
        .collect();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/mutate");
    let mut failures = Vec::new();
    for (targets, seeds) in [(BYTES, &images), (TEXTS, &texts)] {
        for (name, f) in targets {
            let seeds = if *name == "inflate_zlib" { &zlib } else { seeds };
            for n in 0..rounds {
                let (a, b) = (rng.below(seeds.len()), rng.below(seeds.len()));
                let input = mutate(&mut rng, &seeds[a], &seeds[b]);
                if let Err(e) = call(*f, &input) {
                    std::fs::create_dir_all(&out).unwrap();
                    let file = out.join(format!("{}-{n}.bin", name.replace("::", "-")));
                    std::fs::write(&file, &input).unwrap();
                    failures.push(format!("{name}: {e} ({})", file.display()));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "seed {seed}, {rounds} rounds: {} failure(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}
