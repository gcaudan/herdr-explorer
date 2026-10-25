//! The module fixtures recorded from the JS (tools/fixtures/gen.js), replayed against the Rust port. A module not
//! ported yet is ignored with the step that ports it.

mod support;

use herdr_explorer::json::{self, Value};

/// Every fixture file parses, is written back byte for byte (the JSON writer matches JSON.stringify), and each case
/// has a function, arguments and an outcome.
#[test]
fn fixtures_are_well_formed() {
    let mut count = 0;
    for entry in std::fs::read_dir(support::fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let v = json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(format!("{}\n", json::stringify_pretty(&v, 1)), text, "{} written back", path.display());
        for case in v.get("cases").and_then(Value::as_array).unwrap() {
            assert!(case.get("fn").and_then(Value::as_str).is_some(), "{}: fn", path.display());
            assert!(case.get("args").and_then(Value::as_array).is_some(), "{}: args", path.display());
            assert!(case.get("out").is_some() || case.get("throws").is_some(), "{}: outcome", path.display());
            count += 1;
        }
    }
    assert!(count > 0);
}

/// A call that panics where the JS throws: its message as the error.
fn caught(f: impl FnOnce() -> Value) -> Result<Value, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()
}

/// A shell script's output; it must succeed.
fn sh(script: &str) -> String {
    let out = std::process::Command::new("sh").args(["-c", script]).output().unwrap();
    assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn str_arr(items: Vec<String>) -> Value {
    Value::Arr(items.into_iter().map(Value::Str).collect())
}

#[cfg(unix)]
#[test]
fn core_exec() {
    use herdr_explorer::core::exec;
    support::replay("core/exec", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(match name {
            "words" => str_arr(exec::words(&s(0))),
            "version" => Value::Arr(exec::version(&s(0)).into_iter().map(Value::Num).collect()),
            "newer" => Value::Bool(exec::newer(&s(0), &s(1))),
            "addConfig" => {
                let mut env: exec::Env = args[0]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect();
                let pairs: Vec<(String, String)> = args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let p = strs(p);
                        (p[0].clone(), p[1].clone())
                    })
                    .collect();
                let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                exec::add_config(&mut env, &pairs);
                Value::Obj(env.into_iter().map(|(k, v)| (k, Value::Str(v))).collect())
            }
            "result" => exec::result(Some(&s(0))),
            "staleState" => {
                str_arr(exec::stale_state(&s(0), &strs(&args[1]), &strs(&args[2]), &strs(&args[3]), &strs(&args[4])))
            }
            "gitOperation" => Value::Str(exec::git_operation(&s(0)).into()),
            "remoteLink" => Value::Bool(exec::remote_link(&s(0), &s(1))),
            "realPath" => Value::Str(exec::real_path(&s(0))),
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_exec_state() {
    use herdr_explorer::core::exec;
    let bindings = |v: &Value| -> exec::Bindings {
        v.as_object().unwrap().iter().map(|(k, keys)| (k.clone(), strs(keys))).collect()
    };
    support::replay("core/exec.state", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        let opt = |o: Option<String>| o.map_or(Value::Null, Value::Str);
        caught(|| match name {
            "readWidth" => Value::Num(exec::read_width()),
            "readSettings" => exec::read_settings(),
            "writeSettings" => {
                exec::write_settings(&args[0]);
                Value::Null
            }
            // The JS gives '' for an empty id where Rust gives None, both falsy.
            "paneFile" if s(0).is_empty() && exec::state_dir().is_some() => Value::Str(String::new()),
            "paneFile" => opt(exec::pane_file(&s(0))),
            "readPaneState" => exec::read_pane_state(&s(0)),
            "writePaneState" => {
                exec::write_pane_state(&s(0), &args[1]);
                Value::Null
            }
            "tabFile" if s(1).is_empty() && exec::state_dir().is_some() => Value::Str(String::new()),
            "tabFile" => opt(exec::tab_file(&s(0), &s(1))),
            "readKeys" => Value::Arr(
                exec::read_keys(&s(0), &bindings(&args[1]))
                    .0
                    .into_iter()
                    .map(|(k, a)| Value::Arr(vec![Value::Str(k), Value::Str(a)]))
                    .collect(),
            ),
            "keysTemplate" => opt(exec::keys_template(&s(0), &bindings(&args[1]))),
            "writeKeys" => {
                let changes: Vec<(String, Option<Value>)> = args[1]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), (!v.is_null()).then(|| v.clone())))
                    .collect();
                Value::Bool(exec::write_keys(&s(0), &changes))
            }
            "writeAtomic" => Value::Bool(exec::write_atomic(&s(0), s(1))),
            _ => panic!("no function {name}"),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_places() {
    use herdr_explorer::core::places;
    support::replay("core/places", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(match name {
            "target" => {
                let (file, marks) = places::target(&s(0), &s(1))?;
                let marks = marks
                    .iter()
                    .map(|m| {
                        let n = |x: u64| Value::Num(x as f64);
                        herdr_explorer::obj! { "from" => n(m.from), "to" => n(m.to), "a" => n(m.a), "b" => n(m.b) }
                    })
                    .collect();
                Value::Arr(vec![Value::Str(file), Value::Arr(marks)])
            }
            "clicked" => places::clicked(&s(0), &s(1)).map_or(Value::Null, Value::Str),
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_scope() {
    use herdr_explorer::core::exec;
    use herdr_explorer::core::scope::{self, Scope};
    support::replay("core/scope", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        let sc = |i: usize| Scope(args[i].clone());
        Ok(match name {
            "label" => Value::Str(scope::label(&sc(0), &s(1))),
            "itemsOf" => Value::Arr(
                scope::items(&sc(0), &s(1))
                    .into_iter()
                    .map(|it| {
                        it.map_or(Value::Null, |it| {
                            let run = match it.run {
                                scope::Pick::Scope(p) => Value::Arr(vec!["pick".into(), p.0]),
                                scope::Pick::Ask(k) => Value::Arr(vec!["ask".into(), k.into()]),
                            };
                            let value = it.value.map_or(Value::Null, |(t, st)| Value::Arr(vec![t.into(), st.into()]));
                            herdr_explorer::obj! { "label" => it.label, "value" => value, "run" => run }
                        })
                    })
                    .collect(),
            ),
            "revKeyOf" => {
                let mut f = herdr_explorer::core::ui::Field::new(&s(0));
                let steps = args[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| {
                        let r = match scope::rev_key(&mut f, k.as_str().unwrap()) {
                            scope::RevKey::Done(rev) => herdr_explorer::obj! { "done" => true, "rev" => rev },
                            scope::RevKey::Changed(c) => herdr_explorer::obj! { "changed" => c },
                        };
                        Value::Arr(vec![r, f.value.clone().into()])
                    })
                    .collect();
                Value::Arr(steps)
            }
            "same" => Value::Bool(scope::same(&sc(0), &sc(1))),
            "read" => scope::read(&s(0)).0,
            "write" => Value::Bool(scope::write(&s(0), &sc(1))),
            "branchesIn" => {
                sh(&s(0));
                str_arr(scope::branches(&s(1)))
            }
            "worktreeOf" => {
                sh(&s(0));
                let mut env = exec::process_env();
                exec::env_set(&mut env, "GIT_LITERAL_PATHSPECS", "1");
                let only = args.get(3).and_then(Value::as_str);
                let calls = args.get(4).and_then(Value::as_f64).unwrap_or(1.0) as usize;
                let keep = args[2].truthy();
                let trees: Vec<Value> = (0..calls)
                    .map(|_| scope::worktree(&s(1), &env, keep, only).map_or(Value::Null, Value::Str))
                    .collect();
                let dir = std::env::var("HERDR_PLUGIN_STATE_DIR").unwrap_or_default();
                let state = sh(&format!("cd '{dir}' 2>/dev/null && find . -type f | sort || true"));
                let hash =
                    |w: &str| w.len() == 40 && w.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                let state: String = state
                    .split_inclusive(['/', '.', '\n'])
                    .map(|part| {
                        let word = part.trim_end_matches(['/', '.', '\n']);
                        if hash(word) { part.replacen(word, "HASH", 1) } else { part.to_string() }
                    })
                    .collect();
                Value::Obj(vec![("trees".into(), Value::Arr(trees)), ("state".into(), Value::Str(state))])
            }
            "agentStatusIn" => {
                sh(&s(0));
                let steps = args[1].as_array().unwrap().iter().map(strs);
                Value::Arr(
                    steps
                        .map(|st| {
                            let done = scope::agent_status(&st[0], &st[1], &st[2], &st[3]);
                            let mut turn = scope::read_turn(&st[0]);
                            if turn.truthy() {
                                let kind = match turn.get("t") {
                                    None => "undefined",
                                    Some(Value::Num(_)) => "number",
                                    Some(Value::Str(_)) => "string",
                                    Some(Value::Bool(_)) => "boolean",
                                    Some(_) => "object",
                                };
                                turn.set("t", Value::Str(kind.into()));
                            }
                            Value::Arr(vec![Value::Str(done.into()), turn])
                        })
                        .collect(),
                )
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

/// Node's own codecs as the JS calls them (tools/fixtures/modules/node.codec.js): inflate's input and output in base64.
#[cfg(unix)]
#[test]
fn node_codec() {
    use herdr_explorer::codec::{base64, inflate, sha};
    support::replay("node/codec", |name, args| {
        let text = args[0].as_str().unwrap_or("");
        Ok(match name {
            "base64Encode" => Value::Str(base64::encode(text.as_bytes())),
            "base64Decode" => Value::Arr(base64::decode(text).into_iter().map(|b| Value::Num(b as f64)).collect()),
            "sha1" => Value::Str(sha::hex(&sha::sha1(text.as_bytes()))),
            "sha256" => Value::Str(sha::hex(&sha::sha256(text.as_bytes()))),
            "inflate" => {
                let max = args[1].as_f64().unwrap() as usize;
                match inflate::inflate_zlib(&base64::decode(text), max) {
                    Ok(out) => Value::Str(base64::encode(&out)),
                    Err(inflate::Error::TooLarge) => return Err("too large".into()),
                    Err(inflate::Error::Data(_)) => return Err("data".into()),
                }
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

/// Node's path module, posix and win32 (tools/fixtures/modules/node.path.js).
#[cfg(unix)]
#[test]
fn node_path() {
    use herdr_explorer::core::paths::{posix, win32};
    support::replay("node/path", |name, args| {
        let s: Vec<&str> = args.iter().map(|a| a.as_str().unwrap_or("")).collect();
        let suffix = s.get(1).copied();
        Ok(match name {
            "posix.resolve" => Value::Str(posix::resolve(&s)),
            "posix.normalize" => Value::Str(posix::normalize(s[0])),
            "posix.join" => Value::Str(posix::join(&s)),
            "posix.relative" => Value::Str(posix::relative(s[0], s[1])),
            "posix.dirname" => Value::Str(posix::dirname(s[0])),
            "posix.basename" => Value::Str(posix::basename(s[0], suffix)),
            "posix.extname" => Value::Str(posix::extname(s[0])),
            "posix.isAbsolute" => Value::Bool(posix::is_absolute(s[0])),
            "posix.root" => Value::Str(posix::root(s[0])),
            "win32.resolve" => Value::Str(win32::resolve(&s)),
            "win32.normalize" => Value::Str(win32::normalize(s[0])),
            "win32.join" => Value::Str(win32::join(&s)),
            "win32.relative" => Value::Str(win32::relative(s[0], s[1])),
            "win32.dirname" => Value::Str(win32::dirname(s[0])),
            "win32.basename" => Value::Str(win32::basename(s[0], suffix)),
            "win32.extname" => Value::Str(win32::extname(s[0])),
            "win32.isAbsolute" => Value::Bool(win32::is_absolute(s[0])),
            "win32.root" => Value::Str(win32::root(s[0])),
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_tabs() {
    use herdr_explorer::core::tabs::{self, Tab, Tabs};
    // A tab as the JS has it: { file, kept, ...the preview's fields }.
    let tab_of = |v: &Value| Tab {
        file: v.get("file").and_then(Value::as_str).unwrap_or("").to_string(),
        kept: v.get("kept").is_some_and(Value::truthy),
        extra: Value::Obj(v.as_object().unwrap().iter().filter(|(k, _)| k != "file" && k != "kept").cloned().collect()),
    };
    let tabs_of = |v: &Value| Tabs {
        list: v.get("list").and_then(Value::as_array).unwrap().iter().map(tab_of).collect(),
        current: v.get("current").and_then(Value::as_f64).unwrap() as usize,
    };
    let value_of = |t: &Tabs<Value>| {
        let list = t
            .list
            .iter()
            .map(|t| {
                let mut fields =
                    vec![("file".to_string(), Value::Str(t.file.clone())), ("kept".to_string(), Value::Bool(t.kept))];
                fields.extend(t.extra.as_object().cloned().unwrap_or_default());
                Value::Obj(fields)
            })
            .collect();
        herdr_explorer::obj! { "list" => Value::Arr(list), "current" => t.current as f64 }
    };
    let num = |v: &Value| v.as_f64().unwrap_or(0.0) as i64;
    support::replay("core/tabs", |name, args| {
        Ok(match name {
            "open" => {
                let busy = args.get(3).map(strs).unwrap_or_default();
                let file = args[1].as_str().unwrap();
                let o = tabs::open(&tabs_of(&args[0]), file, args[2].truthy(), |t| busy.contains(&t.file));
                let mut v = value_of(&o.tabs);
                v.set("fresh", Value::Bool(o.fresh));
                v
            }
            "close" => {
                let i = num(&args[1]);
                let t = tabs_of(&args[0]);
                value_of(&if i < 0 { t } else { tabs::close(&t, i as usize) })
            }
            "step" => value_of(&tabs::step(&tabs_of(&args[0]), num(&args[1]))),
            "keep" => {
                let t = tabs_of(&args[0]);
                let i = args.get(1).map_or(t.current, |v| num(v) as usize);
                value_of(&tabs::keep(&t, i))
            }
            "labels" => str_arr(tabs::labels(&strs(&args[0]))),
            "strip" => {
                let widths: Vec<i64> = args[0].as_array().unwrap().iter().map(num).collect();
                let s = tabs::strip(&widths, num(&args[1]), num(&args[2]), num(&args[3]));
                herdr_explorer::obj! { "first" => s.first as f64, "last" => s.last as f64, "cut" => s.cut as f64 }
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_viewed() {
    use herdr_explorer::core::viewed;
    support::replay("core/viewed", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(match name {
            "sign" => Value::Str(viewed::sign(&s(0))),
            "read" => viewed::read(&s(0)),
            "is" => Value::Bool(viewed::is(&args[0], &s(1))),
            "toggle" => Value::Bool(viewed::toggle(&s(0), &s(1))),
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[test]
fn node_locale() {
    use herdr_explorer::unicode::locale_compare;
    support::replay("node/locale", |name, args| {
        let words = strs(&args[0]);
        let numeric_base = match name {
            "variant" => false,
            "numericBase" => true,
            _ => return Err(format!("no function {name}")),
        };
        Ok(Value::Arr(
            words
                .iter()
                .map(|a| {
                    let row = words.iter().map(|b| match locale_compare(a, b, numeric_base) {
                        std::cmp::Ordering::Less => '-',
                        std::cmp::Ordering::Equal => '0',
                        std::cmp::Ordering::Greater => '+',
                    });
                    Value::Str(row.collect())
                })
                .collect(),
        ))
    });
}

#[test]
fn core_notes() {
    use herdr_explorer::core::notes::{self, Lines, New};
    support::replay("core/notes", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        let list = |i: usize| args.get(i).and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(match name {
            "read" => Value::Arr(notes::read(&s(0))),
            "locate" => {
                let lines = match &args[1] {
                    Value::Null => Lines::Gone,
                    Value::Str(_) => Lines::Unknown,
                    v => Lines::Lines(strs(v)),
                };
                notes::locate(&args[0], &lines).unwrap_or_else(|| args[0].clone())
            }
            "current" => Value::Arr(notes::current(&s(0), args.get(1).and_then(Value::as_str))),
            "add" => {
                let n = &args[1];
                let text = |k: &str| n.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let num = |k: &str| n.get(k).and_then(Value::as_f64).map(|x| x as u64);
                let note = New {
                    file: text("file"),
                    from: num("from").unwrap_or(0),
                    to: num("to"),
                    kind: text("kind"),
                    text: text("text"),
                    lines: n.get("lines").map(|l| l.as_array().map(|_| strs(l))),
                    stale: n.get("stale").is_some_and(Value::truthy),
                };
                Value::Str(notes::add_at(&s(0), note, 1700000000000.0))
            }
            "undo" => Value::Str(notes::undo(&s(0))),
            "where" => Value::Str(notes::where_(&args[0], Some(&s(1)), args.get(2).is_some_and(Value::truthy))),
            "format" => Value::Str(notes::format(&list(0), Some(&s(1)), &s(2))),
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[test]
fn diff_parse() {
    use herdr_explorer::diff;
    let opt = |s: &Option<String>| s.clone().map_or(Value::Null, Value::Str);
    let num = |n: Option<i64>| n.map_or(Value::Null, |n| Value::Num(n as f64));
    let obj = |fields: Vec<(&str, Value)>| Value::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    support::replay("diff", |name, args| {
        let s = args[0].as_str().unwrap_or("");
        Ok(match name {
            "unquote" => Value::Str(diff::unquote(s)),
            "strip" => opt(&diff::strip(s)),
            "parse" => {
                let d = diff::parse(s);
                let line = |l: &diff::Line| {
                    let mut items =
                        vec![Value::Str(l.sign.to_string()), Value::Str(l.text.clone()), num(l.old), num(l.new)];
                    if l.sign != '\\' {
                        items.extend([Value::Num(l.at as f64), Value::Bool(l.cr)]);
                    }
                    Value::Arr(items)
                };
                let hunk = |h: &diff::Hunk| {
                    obj(vec![
                        ("head", Value::Str(h.head.clone())),
                        ("context", Value::Str(h.context.clone())),
                        ("lines", Value::Arr(h.lines.iter().map(line).collect())),
                    ])
                };
                let file = |f: &diff::File| {
                    obj(vec![
                        ("from", opt(&f.from)),
                        ("to", opt(&f.to)),
                        ("status", Value::Str(f.status.clone())),
                        ("notes", str_arr(f.notes.clone())),
                        ("hunks", Value::Arr(f.hunks.iter().map(hunk).collect())),
                        ("add", Value::Num(f.add as f64)),
                        ("del", Value::Num(f.del as f64)),
                        ("mode", opt(&f.mode)),
                        ("oldMode", opt(&f.old_mode)),
                        ("newMode", opt(&f.new_mode)),
                    ])
                };
                obj(vec![
                    ("pre", str_arr(d.pre.clone())),
                    ("files", Value::Arr(d.files.iter().map(file).collect())),
                    ("add", Value::Num(d.add as f64)),
                    ("del", Value::Num(d.del as f64)),
                ])
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[test]
fn core_stage() {
    use herdr_explorer::core::stage::{self, Mode, Sel};
    use herdr_explorer::diff::{self, Diff};
    use std::process::Command;
    const DIFF_OPTS: [&str; 6] = [
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=all",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    let git = |top: &str, args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(top).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let indices = |v: Option<&Value>| -> Vec<Vec<usize>> {
        let all = v.and_then(Value::as_array).cloned().unwrap_or_default();
        all.iter()
            .map(|x| match x {
                Value::Arr(a) => a.iter().map(|n| n.as_f64().unwrap() as usize).collect(),
                n => vec![n.as_f64().unwrap() as usize],
            })
            .collect()
    };
    // The case's repository, the diffs read from it and the selection, as the fixture's load() makes them.
    let load = |c: &Value| -> (String, Diff, Sel, Mode) {
        let top = c.get("top").and_then(Value::as_str).unwrap().to_string();
        sh(c.get("setup").and_then(Value::as_str).unwrap());
        let mut parsed = Diff::default();
        for d in c.get("diffs").and_then(Value::as_array).cloned().unwrap_or_default() {
            let d = strs(&d);
            let args: Vec<&str> = d[1..].iter().map(String::as_str).collect();
            for mut f in diff::parse(&git(&top, &args)).files {
                f.sec = d[0].clone();
                parsed.files.push(f);
            }
        }
        let sel = c.get("sel").cloned().unwrap_or(Value::Null);
        let sel = Sel {
            files: indices(sel.get("files")).into_iter().map(|i| i[0]).collect(),
            hunks: indices(sel.get("hunks")).into_iter().map(|i| (i[0], i[1])).collect(),
            lines: indices(sel.get("lines")).into_iter().map(|i| (i[0], i[1], i[2])).collect(),
        };
        if let Some(then) = c.get("then").and_then(Value::as_str) {
            sh(then);
        }
        let mode = match c.get("mode").and_then(Value::as_str) {
            Some("stage") => Mode::Stage,
            Some("unstage") => Mode::Unstage,
            _ => Mode::Discard,
        };
        (top, parsed, sel, mode)
    };
    support::replay("core/stage", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(match name {
            "patches" => {
                let (_, parsed, sel, mode) = load(&args[0]);
                let targets = stage::targets(&parsed, &sel, mode);
                Value::Arr(
                    targets.iter().map(|t| stage::patch_of(&parsed, t, mode).map_or(Value::Null, Value::Str)).collect(),
                )
            }
            "count" => {
                let (_, parsed, sel, mode) = load(&args[0]);
                Value::Str(stage::count(&parsed, &sel, mode))
            }
            "apply" => {
                let c = &args[0];
                let (top, parsed, sel, mode) = load(c);
                let trash_dir = format!("{top}/../trash");
                let has_trash = c.get("trash").and_then(Value::as_bool) != Some(false);
                let trash = |p: &str| {
                    if !has_trash {
                        return false;
                    }
                    std::fs::create_dir_all(&trash_dir).unwrap();
                    let name = std::path::Path::new(p).file_name().unwrap();
                    std::fs::rename(p, std::path::Path::new(&trash_dir).join(name)).unwrap();
                    true
                };
                let answer = c.get("noTrash").and_then(Value::as_bool);
                let no_trash = |_: &[String]| answer.unwrap();
                let message =
                    stage::apply(&top, &parsed, &sel, mode, &trash, answer.is_some().then_some(&no_trash as _));
                let mut trashed: Vec<String> = std::fs::read_dir(&trash_dir)
                    .map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
                    .unwrap_or_default();
                trashed.sort();
                let mut diff_args = vec!["diff"];
                diff_args.extend(DIFF_OPTS);
                let mut cached_args = vec!["diff", "--cached"];
                cached_args.extend(DIFF_OPTS);
                Value::Obj(vec![
                    ("message".into(), Value::Str(message)),
                    ("status".into(), Value::Str(git(&top, &["status", "--porcelain=v1", "-uall"]))),
                    ("diff".into(), Value::Str(git(&top, &diff_args))),
                    ("cached".into(), Value::Str(git(&top, &cached_args))),
                    ("trash".into(), str_arr(trashed)),
                ])
            }
            "quote" => Value::Str(stage::quote(&s(0))),
            "within" => Value::Bool(stage::within(&s(0), &s(1))),
            "pruneDirs" => {
                stage::prune_dirs(&s(0), &s(1));
                Value::Str(sh(&format!("cd '{}' && find . | sort", s(1))))
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn core_ui() {
    use herdr_explorer::core::js;
    use herdr_explorer::core::ui::{self, Ask, BarHit, BarMark, Button, Choice, Confirm, Event, Menu, MenuItem};
    use herdr_explorer::core::ui::{Field, HelpState, Kind, Mouse, Rgb, Scrollbar, Seg};
    use herdr_explorer::obj;

    fn at<'a>(v: &'a Value, k: &str) -> &'a Value {
        static NULL: Value = Value::Null;
        v.get(k).unwrap_or(&NULL)
    }
    fn num(v: &Value) -> f64 {
        v.as_f64().unwrap_or(0.0)
    }
    fn s(v: &Value) -> String {
        v.as_str().unwrap_or("").to_string()
    }
    fn opt_s(v: Option<&Value>) -> Option<String> {
        v.and_then(Value::as_str).map(str::to_string)
    }
    fn arr(v: &Value) -> &[Value] {
        v.as_array().map_or(&[], Vec::as_slice)
    }
    fn rgb(v: &Value) -> Rgb {
        let a = arr(v);
        [num(&a[0]) as u8, num(&a[1]) as u8, num(&a[2]) as u8]
    }
    fn rgb_val(c: Rgb) -> Value {
        Value::Arr(c.iter().map(|&x| Value::Num(x as f64)).collect())
    }
    fn segs(v: &Value) -> Vec<Seg> {
        arr(v).iter().map(|p| (s(&arr(p)[0]), arr(p).get(1).map(s).unwrap_or_default())).collect()
    }
    fn seg_val(p: &Seg) -> Value {
        Value::Arr(vec![p.0.clone().into(), p.1.clone().into()])
    }
    fn segs_val(v: &[Seg]) -> Value {
        Value::Arr(v.iter().map(seg_val).collect())
    }
    fn strs_val(v: &[String]) -> Value {
        Value::Arr(v.iter().map(|x| x.clone().into()).collect())
    }
    fn pairs(v: &Value) -> Vec<(String, String)> {
        arr(v).iter().map(|p| (s(&arr(p)[0]), s(&arr(p)[1]))).collect()
    }
    fn mouse(v: &Value) -> Mouse {
        let n = |k: &str| v.get(k).map_or(0, |x| num(x) as i64);
        let b = |k: &str| v.get(k).is_some_and(Value::truthy);
        Mouse {
            button: n("button"),
            mods: n("mods"),
            x: n("x"),
            y: n("y"),
            press: b("press"),
            drag: b("drag"),
            wheel: n("wheel"),
        }
    }
    fn mouse_val(m: &Mouse) -> Value {
        obj! { "button" => m.button, "mods" => m.mods, "x" => m.x, "y" => m.y, "press" => m.press, "drag" => m.drag,
        "wheel" => m.wheel }
    }
    fn first(v: &Value) -> Option<char> {
        v.as_str().and_then(|t| t.chars().next())
    }
    /// A JS string index (UTF-16) of text as a byte offset.
    fn byte_of(text: &str, unit: usize) -> usize {
        let mut u = 0;
        for (b, ch) in text.char_indices() {
            if u >= unit {
                return b;
            }
            u += ch.len_utf16();
        }
        text.len()
    }
    fn kind(name: &str) -> Kind {
        match name {
            "error" => Kind::Error,
            "busy" => Kind::Busy,
            "info" => Kind::Info,
            _ => Kind::Ok,
        }
    }
    fn hint_zones(h: &ui::Hints) -> Value {
        Value::Arr(
            h.zones.iter().map(|z| obj! { "x0" => z.x0 as f64, "x1" => z.x1 as f64, "key" => z.key.clone() }).collect(),
        )
    }
    fn call(name: &str, args: &[Value]) -> Value {
        let a = |i: usize| args.get(i).unwrap_or(&Value::Null);
        match name {
            "shade" => rgb_val(ui::shade(rgb(a(0)))),
            "glyph" => ui::glyph(num(a(0)) as u32).into(),
            "backgroundOf" => ui::background_of(&s(a(0))).map_or(Value::Null, Into::into),
            "clean" => ui::clean(&s(a(0))).into(),
            "cleanLine" => {
                let text = s(a(0));
                let tab = num(a(1)) as usize;
                if !a(2).truthy() {
                    return ui::clean_line(&text, tab, None).into();
                }
                let mut at = Vec::new();
                let out = ui::clean_line(&text, tab, Some(&mut at));
                // The byte offsets as the JS's UTF-16 ones: one per unit of text, then its end.
                let units = |b: usize| js::utf16_len(&out[..b]) as f64;
                let mut js_at = Vec::new();
                for (b, ch) in text.char_indices() {
                    for k in 0..ch.len_utf16() {
                        js_at.push(Value::Num(units(at[b]) + k as f64));
                    }
                }
                js_at.push(Value::Num(units(at[text.len()])));
                Value::Arr(vec![out.into(), Value::Arr(js_at)])
            }
            "cols" => (ui::cols(first(a(0)).unwrap(), first(a(1))) as f64).into(),
            "fit" => {
                let max = if a(1).is_null() { usize::MAX } else { num(a(1)) as usize };
                let text = s(a(0));
                let (part, w) = ui::fit(&text, max, first(a(2)));
                Value::Arr(vec![part.into(), (w as f64).into()])
            }
            "textWidth" => (ui::text_width(&s(a(0))) as f64).into(),
            "ellipsis" => ui::ellipsis(&s(a(0)), num(a(1)) as i64).into(),
            "line" => ui::line(&segs(a(0)), num(a(1)) as usize, (!a(2).is_null()).then(|| rgb(a(2)))).into(),
            "keys" => {
                let max = if a(1).is_null() { usize::MAX } else { num(a(1)) as usize };
                let h = ui::keys(&pairs(a(0)), max);
                obj! { "segs" => segs_val(&h.segs), "zones" => hint_zones(&h) }
            }
            "keyAt" => ui::key_at(&ui::keys(&pairs(a(0)), usize::MAX), num(a(1)) as i64, num(a(2)) as i64)
                .map_or(Value::Null, Into::into),
            "keyLabel" => ui::key_label(&s(a(0))).into(),
            "empty" => strs_val(&ui::empty(num(a(0)) as usize, num(a(1)) as usize, &s(a(2)), &pairs(a(3)))),
            "messageKind" => ui::message_kind(&s(a(0))).name().into(),
            "messageTime" => (ui::message_time(kind(&s(a(0)))) as f64).into(),
            "message" => segs_val(&ui::message(&s(a(0)), a(1).as_str().map(kind))),
            "wrap" => strs_val(&ui::wrap(&s(a(0)), num(a(1)) as i64)),
            "segText" => ui::seg_text(&segs(a(0))).into(),
            "segWidth" => (ui::seg_width(&segs(a(0))) as f64).into(),
            "slice" => segs_val(&ui::slice(&segs(a(0)), num(a(1)) as usize, num(a(2)) as usize)),
            "split" => Value::Arr(ui::split(&segs(a(0)), num(a(1)) as usize).iter().map(|l| segs_val(l)).collect()),
            "mark" => {
                let list = segs(a(0));
                let text = ui::seg_text(&list);
                let ranges: Vec<(usize, usize)> = arr(a(1))
                    .iter()
                    .map(|r| (byte_of(&text, num(&arr(r)[0]) as usize), byte_of(&text, num(&arr(r)[1]) as usize)))
                    .collect();
                segs_val(&ui::mark(&list, &ranges, &s(a(2))))
            }
            "onOff" => seg_val(&ui::on_off(a(0).truthy())),
            "switchStyle" => ui::switch_style(a(0).truthy()).into(),
            "segmented" => {
                let choices: Vec<(String, String)> = pairs(a(0));
                let refs: Vec<(&str, &str)> = choices.iter().map(|(x, y)| (x.as_str(), y.as_str())).collect();
                let (sg, width, hits) = ui::segmented(&refs, &s(a(1)));
                let hits = hits
                    .iter()
                    .map(|h| obj! { "id" => h.id.clone(), "from" => h.from as f64, "to" => h.to as f64 })
                    .collect();
                obj! { "segs" => segs_val(&sg), "width" => width as f64, "hits" => Value::Arr(hits) }
            }
            "head" => ui::head(&s(a(0)), num(a(1)) as usize).into(),
            "tail" => ui::tail(&s(a(0)), num(a(1)) as usize).into(),
            "keyName" => ui::key_name(&s(a(0))).map_or(Value::Null, Into::into),
            "printable" => ui::printable(&s(a(0))).into(),
            "decode" => Value::Arr(
                ui::decode(&s(a(0)))
                    .iter()
                    .map(|e| match e {
                        Event::Key(k) => Value::Arr(vec!["key".into(), k.clone().into()]),
                        Event::Mouse(m) => Value::Arr(vec!["mouse".into(), mouse_val(m)]),
                        Event::Focus(f) => Value::Arr(vec!["focus".into(), (*f).into()]),
                        Event::Paste { text, raw } => {
                            Value::Arr(vec!["paste".into(), Value::Arr(vec![text.clone().into(), raw.clone().into()])])
                        }
                        Event::Reply(r) => Value::Arr(vec!["reply".into(), r.clone().into()]),
                    })
                    .collect(),
            ),
            "field" => {
                let mut f = Field::new(&s(a(0)));
                let steps = arr(a(1))
                    .iter()
                    .map(|k| {
                        let k = s(k);
                        let changed = match k.strip_prefix("insert:") {
                            Some(t) => {
                                f.insert(t);
                                true
                            }
                            None => f.key(&k),
                        };
                        Value::Arr(vec![changed.into(), f.value.clone().into(), (f.cursor as f64).into()])
                    })
                    .collect();
                obj! { "steps" => Value::Arr(steps), "segments" => segs_val(&f.segments(num(a(2)) as i64, &s(a(3)))) }
            }
            "help" => {
                let (w, h) = (num(a(0)) as usize, num(a(1)) as usize);
                let title = s(a(2));
                let groups: Vec<ui::Group> = arr(a(3)).iter().map(|g| (s(&arr(g)[0]), pairs(&arr(g)[1]))).collect();
                let mut state = HelpState::default();
                let draw = |state: &mut HelpState| {
                    let mut lines = vec![String::new(); h];
                    ui::help(&mut lines, w, &title, &groups, state);
                    strs_val(&lines)
                };
                let mut out = vec![draw(&mut state)];
                for step in arr(a(4)) {
                    let (kind, v) = (s(&arr(step)[0]), &arr(step)[1]);
                    let open = if kind == "key" {
                        ui::help_key(&s(v), &mut state)
                    } else {
                        ui::help_mouse(&mouse(v), &mut state)
                    };
                    let find = state.find.as_ref().map_or(Value::Null, |f| f.value.clone().into());
                    let lines = if open { draw(&mut state) } else { Value::Null };
                    out.push(Value::Arr(vec![open.into(), (state.top as f64).into(), find, lines]));
                }
                Value::Arr(out)
            }
            "confirm" => {
                let (w, h) = (num(a(0)) as i64, num(a(1)) as i64);
                let q0 = a(2);
                let mut q = Confirm {
                    title: s(at(q0, "title")),
                    name: s(at(q0, "name")),
                    where_: opt_s(q0.get("where")),
                    note: opt_s(q0.get("note")),
                    action: s(at(q0, "action")),
                    tone: opt_s(q0.get("tone")).unwrap_or_default(),
                    focus: s(at(q0, "focus")),
                    zones: None,
                };
                let mut lines: Vec<String> = (0..h).map(|_| " ".repeat(w as usize)).collect();
                ui::confirm(&mut lines, w, h, &mut q);
                let pair = |p: (i64, i64)| Value::Arr(vec![p.0.into(), p.1.into()]);
                let zones = q
                    .zones
                    .as_ref()
                    .map_or(Value::Null, |z| obj! { "y" => z.y, "yes" => pair(z.yes), "no" => pair(z.no) });
                let clicks = arr(a(4))
                    .iter()
                    .map(|m| ui::confirm_mouse(&mouse(m), &q).map_or(Value::Null, Into::into))
                    .collect();
                let keys = arr(a(3))
                    .iter()
                    .map(|k| {
                        let r = ui::confirm_key(&s(k), &mut q);
                        Value::Arr(vec![r.into(), q.focus.clone().into()])
                    })
                    .collect();
                obj! { "lines" => strs_val(&lines), "zones" => zones, "clicks" => Value::Arr(clicks), "keys" => Value::Arr(keys) }
            }
            "ask" => {
                let (w, h) = (num(a(0)) as i64, num(a(1)) as i64);
                let q0 = a(2);
                let buttons = arr(at(q0, "buttons"))
                    .iter()
                    .map(|b| Button {
                        key: s(at(b, "key")),
                        label: s(at(b, "label")),
                        tone: opt_s(b.get("tone")).unwrap_or_default(),
                    })
                    .collect();
                let mut q = Ask {
                    title: s(at(q0, "title")),
                    name: s(at(q0, "name")),
                    where_: opt_s(q0.get("where")),
                    note: opt_s(q0.get("note")),
                    list: arr(at(q0, "list")).iter().map(s).collect(),
                    buttons,
                    focus: num(at(q0, "focus")) as usize,
                    ..Ask::default()
                };
                let mut lines: Vec<String> = (0..h).map(|_| " ".repeat(w as usize)).collect();
                ui::ask(&mut lines, w, h, &mut q);
                let zones = q.zones.as_ref().map_or(Value::Null, |z| {
                    Value::Arr(
                        z.iter()
                            .map(|(x0, x1, k, y)| {
                                Value::Arr(vec![(*x0).into(), (*x1).into(), k.clone().into(), (*y).into()])
                            })
                            .collect(),
                    )
                });
                let boxed = q
                    .boxed
                    .map_or(Value::Null, |(x0, x1, y0, y1)| obj! { "x0" => x0, "x1" => x1, "y0" => y0, "y1" => y1 });
                let clicks =
                    arr(a(4)).iter().map(|m| ui::ask_mouse(&mouse(m), &q).map_or(Value::Null, Into::into)).collect();
                let keys = arr(a(3))
                    .iter()
                    .map(|k| {
                        let r = ui::ask_key(&s(k), &mut q).map_or(Value::Null, Into::into);
                        Value::Arr(vec![r, (q.focus as f64).into()])
                    })
                    .collect();
                obj! { "lines" => strs_val(&lines), "zones" => zones, "box" => boxed, "clicks" => Value::Arr(clicks),
                "keys" => Value::Arr(keys) }
            }
            "scrollbar" => {
                let (total, height, top) = (num(a(0)) as i64, num(a(1)) as i64, num(a(2)) as i64);
                let marks: Vec<BarMark> = arr(a(3))
                    .iter()
                    .map(|m| BarMark {
                        row: num(at(m, "row")) as i64,
                        color: rgb(at(m, "color")),
                        weight: num(at(m, "weight")),
                    })
                    .collect();
                let mut bar = Scrollbar::new();
                let cells = |bar: &Scrollbar| strs_val(&(0..height).map(|k| bar.cell(k)).collect::<Vec<_>>());
                let cols = bar.set(total, height, top, &marks);
                let first = cells(&bar);
                let top_at = if bar.bar.is_some() {
                    Value::Arr((0..height).map(|r| bar.top_at(r).into()).collect())
                } else {
                    Value::Null
                };
                let now = std::time::Instant::now();
                let mut steps = Vec::new();
                for step in arr(a(4)) {
                    let st = arr(step);
                    let hit = bar.mouse(&mouse(&st[0]), num(&st[1]) as i64, st[2].truthy(), now);
                    let (took, scrolled) = match hit {
                        BarHit::Ignored => (false, Value::Null),
                        BarHit::Took => (true, Value::Null),
                        BarHit::Scroll(t) => {
                            bar.set(total, height, t, &marks);
                            (true, t.into())
                        }
                    };
                    steps.push(Value::Arr(vec![took.into(), scrolled, cells(&bar)]));
                }
                obj! { "cols" => cols as f64, "cells" => first, "topAt" => top_at, "steps" => Value::Arr(steps) }
            }
            "menu" => {
                let o = a(0);
                let items: Vec<Option<MenuItem<String>>> = arr(at(o, "items"))
                    .iter()
                    .map(|it| {
                        (!it.is_null()).then(|| MenuItem {
                            label: s(at(it, "label")),
                            key: opt_s(it.get("key")).unwrap_or_default(),
                            value: it.get("value").map(|v| (s(&arr(v)[0]), s(&arr(v)[1]))),
                            danger: it.get("danger").is_some_and(Value::truthy),
                            keep: it.get("keep").is_some_and(Value::truthy),
                            run: s(at(it, "run")),
                        })
                    })
                    .collect();
                let n = |k: &str, d: i64| o.get(k).map_or(d, |v| num(v) as i64);
                let mut m = Menu::new(&s(at(o, "title")), items, n("at", 0), n("right", 0), n("left", 1));
                let base: Vec<String> = arr(a(1)).iter().map(s).collect();
                let (w, h) = (num(a(2)) as i64, num(a(3)) as i64);
                let draw = |m: &mut Menu<String>| {
                    let mut l = base.clone();
                    m.draw(&mut l, w, h);
                    strs_val(&l)
                };
                let mut out = vec![draw(&mut m), (m.x as f64).into(), (m.inner as f64).into()];
                for step in arr(a(4)) {
                    let (kind, v) = (s(&arr(step)[0]), &arr(step)[1]);
                    let r = match kind.as_str() {
                        "key" => m.key(&s(v), h),
                        "move" => {
                            m.move_by(num(v) as i64, h);
                            None
                        }
                        _ => m.mouse(&mouse(v), h),
                    };
                    let r = match r {
                        None => Value::Null,
                        Some(Choice::Close) => "close".into(),
                        Some(Choice::Run { run, keep }) => obj! { "run" => run, "keep" => keep },
                    };
                    out.push(Value::Arr(vec![r, (m.sel as f64).into(), (m.off as f64).into(), draw(&mut m)]));
                }
                Value::Arr(out)
            }
            "light" => {
                ui::use_theme("light");
                let out = std::panic::catch_unwind(|| call(&s(a(0)), &args[1..]));
                ui::use_theme("dark");
                out.unwrap_or_else(|e| std::panic::resume_unwind(e))
            }
            _ => panic!("no function {name}"),
        }
    }
    support::replay("core/ui", |name, args| caught(|| call(name, args)));
}
