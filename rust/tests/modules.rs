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

/// Node's regular expressions, as the modules use them (tools/fixtures/modules/node.regexp.js).
#[test]
fn node_regexp() {
    use herdr_explorer::core::{js, regex::Regex};
    let opt = |g: Option<&str>| g.map_or(Value::Null, |g| Value::Str(g.into()));
    support::replay("node/regexp", |name, args| {
        let s = |k: usize| args[k].as_str().unwrap_or("");
        let text = s(2);
        let Ok(re) = Regex::new(s(0), s(1)) else { return Ok(Value::Str("SyntaxError".into())) };
        // Positions in UTF-16 units, as the JS counts them.
        let at = |i: usize| Value::Num(js::utf16_len(&text[..i]) as f64);
        Ok(match name {
            "exec" => {
                // From a position in UTF-16 units, as a byte offset.
                let units = args[3].as_f64().unwrap() as usize;
                let from = text
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain([text.len()])
                    .find(|&i| js::utf16_len(&text[..i]) >= units);
                match from.and_then(|from| re.exec_at(text, from)) {
                    None => Value::Null,
                    Some(m) => {
                        Value::Arr([at(m.start())].into_iter().chain((0..m.len()).map(|n| opt(m.get(n)))).collect())
                    }
                }
            }
            "split" => Value::Arr(re.split(text).into_iter().map(opt).collect()),
            "all" => Value::Arr(
                re.matches(text)
                    .iter()
                    .map(|m| Value::Arr(vec![at(m.start()), Value::Str(m.as_str().into())]))
                    .collect(),
            ),
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
    use herdr_explorer::core::{js, ui};
    use herdr_explorer::diff;
    let opt = |s: &Option<String>| s.clone().map_or(Value::Null, Value::Str);
    let num = |n: Option<i64>| n.map_or(Value::Null, |n| Value::Num(n as f64));
    let obj = |fields: Vec<(&str, Value)>| Value::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let diff_val = |d: &diff::Diff| {
        let line = |l: &diff::Line| {
            let mut items = vec![Value::Str(l.sign.to_string()), Value::Str(l.text.clone()), num(l.old), num(l.new)];
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
    };
    // A diff given as text (parse) or as { staged, text } (changes_of, the sections apart).
    let parsed = |v: &Value| match v.as_str() {
        Some(t) => diff::parse(t),
        None => diff::changes_of(&diff::Collected {
            text: v.get("text").map(js::string).unwrap_or_default(),
            staged: v.get("staged").map(js::string),
            ..diff::Collected::default()
        }),
    };
    let ref_of = |v: &Value| diff::Ref {
        file: v.get("file").map(js::string).unwrap_or_default(),
        from: v.get("from").and_then(Value::as_f64).unwrap_or(0.0) as i64,
        to: v.get("to").and_then(Value::as_f64).unwrap_or(0.0) as i64,
        sec: v.get("sec").map(js::string).unwrap_or_default(),
    };
    let ref_val = |r: &diff::Ref| {
        obj(vec![
            ("file", Value::Str(r.file.clone())),
            ("from", Value::Num(r.from as f64)),
            ("to", Value::Num(r.to as f64)),
            ("sec", Value::Str(r.sec.clone())),
        ])
    };
    support::replay("diff", |name, args| {
        let s = args[0].as_str().unwrap_or("");
        Ok(match name {
            "unquote" => Value::Str(diff::unquote(s)),
            "strip" => opt(&diff::strip(s)),
            "parse" => diff_val(&diff::parse(s)),
            "render" => {
                let d = parsed(&args[0]);
                let (width, mode) = (args[1].as_f64().unwrap_or(0.0) as usize, args[2].as_str().unwrap_or(""));
                let view = &args[4];
                let names: Vec<String> = view
                    .get("viewed")
                    .and_then(Value::as_array)
                    .map_or(Vec::new(), |a| a.iter().map(js::string).collect());
                let viewed =
                    |f: &diff::File| names.contains(&f.to.clone().or_else(|| f.from.clone()).unwrap_or_default());
                let v = diff::View {
                    wrap: view.get("wrap").is_some_and(Value::truthy),
                    left: view.get("left").and_then(Value::as_f64).unwrap_or(0.0) as usize,
                    viewed: (!names.is_empty()).then_some(&viewed as &dyn Fn(&diff::File) -> bool),
                };
                let rows = diff::render(&d, width, mode, args[3].truthy(), &v);
                // The JS throws there (String.prototype.repeat of -1): the Rust lays the rows out without failing.
                if width == 0 && mode == "split" {
                    return Err("Invalid count value: -1".into());
                }
                let n = |x: usize| Value::Num(x as f64);
                let segs = |v: &[ui::Seg]| {
                    Value::Arr(v.iter().map(|p| Value::Arr(vec![p.0.clone().into(), p.1.clone().into()])).collect())
                };
                Value::Arr(
                    rows.iter()
                        .map(|r| {
                            obj(vec![
                                ("segs", segs(&r.segs)),
                                (
                                    "back",
                                    r.back.map_or(Value::Null, |c| {
                                        Value::Arr(c.iter().map(|&x| n(x as usize)).collect())
                                    }),
                                ),
                                ("hunk", Value::Bool(r.hunk)),
                                ("file", Value::Bool(r.file)),
                                ("more", Value::Bool(r.more)),
                                ("place", Value::Num(r.place as f64)),
                                ("ref", r.r#ref.as_ref().map_or(Value::Null, ref_val)),
                                ("f", r.f.map_or(Value::Null, n)),
                                ("h", r.h.map_or(Value::Null, |(a, b)| Value::Arr(vec![n(a), n(b)]))),
                                (
                                    "lines",
                                    Value::Arr(
                                        r.lines.iter().map(|&(a, b, c)| Value::Arr(vec![n(a), n(b), n(c)])).collect(),
                                    ),
                                ),
                                ("redo", r.redo.as_ref().map_or(Value::Null, |redo| segs(&redo(7)))),
                            ])
                        })
                        .collect(),
                )
            }
            "changesOf" => {
                let d = parsed(&args[0]);
                let mut v = diff_val(&d);
                // sec, only with the sections apart (undefined in the JS otherwise).
                if let (true, Value::Obj(fields)) = (args[0].get("staged").is_some(), &mut v) {
                    for (k, val) in fields.iter_mut() {
                        if let (true, Value::Arr(files)) = (k == "files", val) {
                            for (f, file) in files.iter_mut().zip(&d.files) {
                                f.set("sec", Value::Str(file.sec.clone()));
                            }
                        }
                    }
                }
                v
            }
            "shownLines" => diff::shown_lines(&parsed(&args[0]), &ref_of(&args[1])).map_or(Value::Null, str_arr),
            "diskLines" => diff::disk_lines(&parsed(&args[0]), &ref_of(&args[1])).map_or(Value::Null, |(from, to)| {
                obj(vec![("from", Value::Num(from as f64)), ("to", Value::Num(to as f64))])
            }),
            "widestOf" => Value::Num(diff::widest_of(&parsed(&args[0])) as f64),
            "defaultMode" => Value::Str(diff::default_mode(args[0].as_f64().unwrap_or(0.0) as usize).into()),
            "showArgs" => diff::show_args(&args[0]).map_or(Value::Null, str_arr),
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

#[test]
fn core_buffer() {
    use herdr_explorer::core::buffer::{self, Editor, LineDiff, Match, Pos};
    use herdr_explorer::obj;

    fn n(v: &Value) -> usize {
        v.as_f64().unwrap_or(0.0) as usize
    }
    fn i(v: &Value) -> i64 {
        v.as_f64().unwrap_or(0.0) as i64
    }
    fn s(v: &Value) -> String {
        v.as_str().unwrap_or("").to_string()
    }
    fn num(x: usize) -> Value {
        Value::Num(x as f64)
    }
    fn doc(d: Result<buffer::Doc, String>) -> Value {
        match d {
            Ok(d) => {
                obj! { "lines" => d.lines, "eol" => d.eol, "final" => d.final_, "bom" => d.bom, "mixed" => d.mixed }
            }
            Err(e) => obj! { "error" => e },
        }
    }
    fn diff(d: &LineDiff) -> Value {
        let kinds: Vec<&str> = d.kinds.iter().map(|k| k.name()).collect();
        obj! { "kinds" => kinds, "deleted" => Value::Arr(d.deleted.iter().map(|&x| num(x)).collect()) }
    }
    fn found(m: Match) -> Value {
        obj! { "line" => num(m.line), "a" => num(m.a), "b" => num(m.b) }
    }
    fn pos(v: &Value) -> Pos {
        Pos::new(n(v.get("line").unwrap()), n(v.get("at").unwrap()))
    }
    fn place(p: Option<Pos>) -> Value {
        p.map_or(Value::Null, |p| obj! { "line" => num(p.line), "at" => num(p.at) })
    }
    // Each code point's start, and the end.
    fn starts(text: &str) -> Vec<usize> {
        text.char_indices().map(|(i, _)| i).chain([text.len()]).collect()
    }
    fn input(v: &Value) -> Vec<u8> {
        match v {
            Value::Arr(a) => a.iter().map(|b| n(b) as u8).collect(),
            v => s(v).into_bytes(),
        }
    }

    support::replay("core/buffer", |name, args| {
        let a = |k: usize| s(&args[k]);
        Ok(match name {
            "parse" => doc(buffer::parse(&input(&args[0]))),
            "roundOf" => match buffer::parse(&input(&args[0])) {
                Ok(d) => Value::Str(d.serialize()),
                Err(e) => obj! { "error" => e },
            },
            "readOf" => doc(buffer::read(&a(0)).map(|(d, _)| d)),
            "saveOf" => {
                buffer::save(&a(1), &a(2)).map_err(|e| e.to_string())?;
                let mut files: Vec<String> =
                    std::fs::read_dir(a(0)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
                files.sort();
                obj! { "files" => files, "text" => std::fs::read_to_string(a(1)).unwrap() }
            }
            "stops" => {
                let t = a(0);
                let mut out = Vec::new();
                let mut k = 0;
                while k < t.len() {
                    out.push(num(k));
                    k = buffer::next_at(&t, k);
                }
                Value::Arr(out)
            }
            "around" => {
                let t = a(0);
                let at = |k: usize| {
                    vec![k, buffer::prev_at(&t, k), buffer::snap(&t, k), buffer::next_at(&t, k)].into_iter().map(num)
                };
                Value::Arr(starts(&t).into_iter().map(|k| Value::Arr(at(k).collect())).collect())
            }
            "shown" => {
                let t = a(0);
                let sh = buffer::shown(&t);
                let at = starts(&t).into_iter().map(|k| Value::Arr(vec![num(k), num(sh.col[k]), num(sh.map[k])]));
                obj! { "text" => sh.text, "width" => num(sh.width), "at" => Value::Arr(at.collect()) }
            }
            "atCol" => num(buffer::at_col(&a(0), n(&args[1]))),
            "colOf" => num(buffer::col_of(&a(0), n(&args[1]))),
            "words" => {
                let t = a(0);
                let at = |k| Value::Arr(vec![num(k), num(buffer::word_start(&t, k)), num(buffer::word_end(&t, k))]);
                Value::Arr(starts(&t).into_iter().map(at).collect())
            }
            "indentOf" => Value::Str(buffer::indent_of(&strs(&args[0]))),
            "lineDiff" => diff(&buffer::line_diff(&strs(&args[0]), &strs(&args[1]), n(&args[2]))),
            "matches" => Value::Arr(buffer::matches(&strs(&args[0]), &a(1)).into_iter().map(found).collect()),
            "edit" => {
                let mut ed = Editor::new(&buffer::parse(a(0).as_bytes()).unwrap());
                let steps = args[1].as_array().unwrap();
                let out = steps.iter().map(|step| {
                    let st = step.as_array().unwrap();
                    let arg = |k: usize| st.get(k + 1).cloned().unwrap_or_default();
                    let ret = match s(&st[0]).as_str() {
                        "place" => {
                            ed.place(pos(&arg(0)), arg(1).truthy());
                            Value::Null
                        }
                        "move" => {
                            ed.move_(&s(&arg(0)), arg(1).truthy(), n(&arg(2)));
                            Value::Null
                        }
                        "goto" => {
                            ed.goto(n(&arg(0)));
                            Value::Null
                        }
                        "selectWord" => {
                            ed.select_word(pos(&arg(0)));
                            Value::Null
                        }
                        "selectLine" => {
                            ed.select_line(pos(&arg(0)));
                            Value::Null
                        }
                        "selectAll" => {
                            ed.select_all();
                            Value::Null
                        }
                        "markSaved" => {
                            ed.mark_saved();
                            Value::Null
                        }
                        "reset" => {
                            ed.reset(&buffer::parse(s(&arg(0)).as_bytes()).unwrap());
                            Value::Null
                        }
                        "type" => {
                            let kind = if s(&arg(1)) == "type" { "type" } else { "" };
                            Value::Bool(ed.type_(&s(&arg(0)), kind))
                        }
                        "newline" => Value::Bool(ed.newline()),
                        "erase" => Value::Bool(ed.erase(i(&arg(0)), arg(1).truthy())),
                        "indentLines" => Value::Bool(ed.indent_lines(i(&arg(0)))),
                        "undo" => Value::Bool(ed.undo()),
                        "redo" => Value::Bool(ed.redo()),
                        "find" => ed.find(&s(&arg(0)), i(&arg(1)), arg(2).truthy()).map_or(Value::Null, found),
                        "replaceOne" => Value::Bool(ed.replace_one(&s(&arg(0)), &s(&arg(1)))),
                        "replaceAll" => num(ed.replace_all(&s(&arg(0)), &s(&arg(1)))),
                        "changed" => diff(ed.changed()),
                        "serialize" => Value::Str(ed.serialize()),
                        other => panic!("edit step {other}"),
                    };
                    let state = vec![
                        Value::from(ed.lines.clone()),
                        place(Some(ed.cur)),
                        place(ed.anchor),
                        Value::Bool(ed.dirty()),
                        Value::Str(ed.selected()),
                        Value::Str(ed.indent.clone()),
                    ];
                    Value::Arr([vec![ret], state].concat())
                });
                Value::Arr(out.collect())
            }
            other => panic!("core/buffer: {other}"),
        })
    });
}

#[test]
fn core_icons() {
    use herdr_explorer::core::{icons, ui};
    use herdr_explorer::obj;

    const ITEMS: &[(&str, bool, bool)] = &[
        ("a.js", false, false),
        ("b.TS", false, false),
        ("README.md", false, false),
        ("Dockerfile", false, false),
        (".gitignore", false, false),
        ("x.unknown", false, false),
        ("noext", false, false),
        ("constructor", false, false),
        ("src", true, false),
        ("src", true, true),
        ("x.d.ts", false, false),
        ("Makefile", false, false),
        ("a.svg", false, false),
        (".git", true, false),
        ("Node_Modules", true, true),
        ("docs", true, false),
        ("icon.png", false, false),
    ];
    fn rgb(c: Option<[u8; 3]>) -> Value {
        c.map_or(Value::Null, |c| Value::Arr(c.iter().map(|&x| Value::Num(x as f64)).collect()))
    }
    fn pair((code, c): (Option<u32>, Option<[u8; 3]>)) -> Value {
        Value::Arr(vec![code.map_or(Value::Null, |g| Value::Num(g as f64)), rgb(c)])
    }

    support::replay("core/icons", |name, args| {
        Ok(match name {
            "iconsOf" => {
                ui::use_glyphs(args[0].as_str().unwrap());
                ui::use_theme(args[1].as_str().unwrap());
                let loaded = if args[2].truthy() { icons::load(&args[2]) } else { None };
                let out = ITEMS
                    .iter()
                    .map(|&(n, dir, open)| {
                        let (text, c) = icons::icon(loaded.as_ref(), n, dir, open);
                        Value::Arr(vec![Value::Str(text), rgb(Some(c))])
                    })
                    .collect();
                ui::use_glyphs("nerd");
                ui::use_theme("dark");
                Value::Arr(out)
            }
            "themesIn" => Value::Arr(
                icons::themes()
                    .into_iter()
                    .map(|t| {
                        let mut fields =
                            vec![("id".to_string(), Value::Str(t.id)), ("label".into(), Value::Str(t.label))];
                        fields.extend(t.version.map(|v| ("version".to_string(), v)));
                        fields.push(("file".into(), Value::Str(t.file)));
                        Value::Obj(fields)
                    })
                    .collect(),
            ),
            "loadOf" => match icons::load(&args[0]) {
                None => Value::Null,
                Some(t) => {
                    let folders: Vec<Value> = strs(&args[1]).iter().map(|n| pair(t.folder(n))).collect();
                    let files: Vec<Value> =
                        strs(&args[2]).iter().map(|n| t.file(n).map_or(Value::Arr(vec![]), pair)).collect();
                    obj! { "entry" => t.entry.clone(), "folders" => folders, "files" => files }
                }
            },
            other => panic!("core/icons: {other}"),
        })
    });
}

/// render/graph: the lanes of histories (tools/fixtures/modules/render.graph.js).
#[test]
fn render_graph() {
    use herdr_explorer::render::graph;
    support::replay("render/graph", |name, args| {
        let commits: Vec<(String, Vec<String>)> = args[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let s = |v: &Value| v.as_str().unwrap().to_string();
                (s(c.get("hash").unwrap()), c.get("parents").and_then(Value::as_array).unwrap().iter().map(s).collect())
            })
            .collect();
        let base = args.get(1).and_then(Value::as_str);
        let num = |n: usize| Value::Num(n as f64);
        Ok(match name {
            "layout" => Value::Arr(
                graph::layout(&commits, base)
                    .into_iter()
                    .map(|r| {
                        let cells = r.cells.iter().map(|&(ch, k)| Value::Arr(vec![Value::Str(ch.to_string()), k.map_or(Value::Null, num)]));
                        herdr_explorer::obj! { "cells" => Value::Arr(cells.collect()), "color" => num(r.color), "above" => Value::Bool(r.above), "below" => Value::Bool(r.below) }
                    })
                    .collect(),
            ),
            _ => return Err(format!("no function {name}")),
        })
    });
}

/// render/image: headers of every format, pixels of PNG, GIF, BMP and ICO files built byte by byte
/// (tools/fixtures/modules/render.image.js).
#[test]
fn render_image() {
    use herdr_explorer::codec::base64;
    use herdr_explorer::render::image;
    support::replay("render/image", |name, args| {
        if name == "crop" {
            let n = |k: usize| args[k].as_f64().unwrap() as i64;
            return Ok(image::crop(n(0), n(1), n(2), n(3), n(4)).map_or(Value::Null, |p| {
                herdr_explorer::obj! { "at" => Value::Num(p.at as f64), "from" => Value::Num(p.from as f64), "count" => Value::Num(p.count as f64), "y" => Value::Num(p.y as f64), "h" => Value::Num(p.h as f64) }
            }));
        }
        let buf = base64::decode(args[0].as_str().unwrap());
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let data = |d: &[u8]| {
            if d.len() <= 512 {
                Value::Str(hex(d))
            } else {
                let fnv = d.iter().fold(0x811c9dc5u32, |h, &x| (h ^ x as u32).wrapping_mul(0x01000193));
                herdr_explorer::obj! { "size" => Value::Num(d.len() as f64), "fnv" => Value::Str(format!("{fnv:x}")), "head" => Value::Str(hex(&d[..64])) }
            }
        };
        Ok(match name {
            "info" => image::info(&buf).map_or(Value::Null, |m| {
                let mut o = herdr_explorer::obj! { "format" => Value::Str(m.format.into()), "width" => Value::Num(m.width), "height" => Value::Num(m.height) };
                if let (Some(note), Value::Obj(o)) = (m.note, &mut o) {
                    o.push(("note".into(), Value::Str(note)));
                }
                o
            }),
            "decode" => image::decode(&buf).map_or(Value::Null, |img| {
                herdr_explorer::obj! { "width" => Value::Num(img.width as f64), "height" => Value::Num(img.height as f64), "data" => data(&img.data) }
            }),
            "render" => {
                let bg = args[3].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as u8).collect::<Vec<_>>();
                let num = |k: usize| args[k].as_f64().unwrap();
                match image::render(&buf, num(1), num(2), Some([bg[0], bg[1], bg[2]])) {
                    Err(info) => herdr_explorer::obj! { "info" => Value::Str(info) },
                    Ok(r) => {
                        let seg = |(t, s): &(String, String)| Value::Arr(vec![Value::Str(t.clone()), Value::Str(s.clone())]);
                        let lines = r.lines.iter().map(|l| Value::Arr(l.iter().map(seg).collect())).collect();
                        herdr_explorer::obj! { "lines" => Value::Arr(lines), "width" => Value::Num(r.width as f64), "height" => Value::Num(r.height as f64), "imageWidth" => Value::Num(r.image_width as f64), "imageHeight" => Value::Num(r.image_height as f64), "format" => Value::Str(r.format.into()) }
                    }
                }
            }
            "pixels" => match image::pixels(&buf, args[1].as_f64().unwrap(), args[2].as_f64().unwrap()) {
                Err(info) => herdr_explorer::obj! { "info" => Value::Str(info) },
                Ok(p) => herdr_explorer::obj! { "width" => Value::Num(p.width as f64), "height" => Value::Num(p.height as f64), "data" => data(&p.data), "imageWidth" => Value::Num(p.image_width as f64), "imageHeight" => Value::Num(p.image_height as f64) },
            },
            _ => return Err(format!("no function {name}")),
        })
    });
}

/// render/highlight: samples of each language in both themes, languages of file names
/// (tools/fixtures/modules/render.highlight.js).
#[test]
fn render_highlight() {
    use herdr_explorer::core::ui;
    use herdr_explorer::render::highlight;
    let segs = |lines: Vec<Vec<(String, String)>>| {
        Value::Arr(
            lines
                .into_iter()
                .map(|l| {
                    Value::Arr(l.into_iter().map(|(t, s)| Value::Arr(vec![Value::Str(t), Value::Str(s)])).collect())
                })
                .collect(),
        )
    };
    support::replay("render/highlight", |name, args| {
        let s = |k: usize| args[k].as_str().unwrap_or("");
        Ok(match name {
            "highlight" => {
                ui::use_theme(s(0));
                let out = highlight::highlight(s(1), args[2].as_str());
                ui::use_theme("dark");
                segs(out)
            }
            "language" => highlight::language(s(0)).map_or(Value::Null, |id| Value::Str(id.into())),
            "styles" => {
                ui::use_theme(s(0));
                let all =
                    highlight::styles().all().iter().map(|(k, v)| (k.to_string(), Value::Str(v.clone()))).collect();
                ui::use_theme("dark");
                Value::Obj(all)
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

/// render/markdown: documents at several widths, both themes, with and without highlighting, images drawn by the
/// caller (tools/fixtures/modules/render.markdown.js).
#[test]
fn render_markdown() {
    use herdr_explorer::core::ui;
    use herdr_explorer::render::markdown::{self, Line, Options};
    use std::rc::Rc;
    // An image of 2 rows for a.png, none for none.png, a failure (none) for throw.png.
    let mut image = |url: &str, width: usize| -> Option<Vec<Line>> {
        (url == "a.png").then(|| {
            (0..2usize)
                .map(|row| Line {
                    segs: vec![(" ".into(), String::new()), (" ".repeat(width.min(4)), String::new())],
                    anchor: None,
                    figure: Some(Rc::new((url.to_string(), row))),
                })
                .collect()
        })
    };
    let image = std::cell::RefCell::new(&mut image);
    support::replay("render/markdown", |name, args| {
        if name != "render" {
            return Err(format!("no function {name}"));
        }
        let opts = args.get(3);
        let flag = |k: &str| opts.and_then(|o| o.get(k)).is_some_and(|v| v.truthy());
        ui::use_theme(args[0].as_str().unwrap_or(""));
        let mut image = image.borrow_mut();
        let res = markdown::render(
            args[1].as_str().unwrap_or(""),
            args[2].as_f64().unwrap_or(0.0),
            Options { highlight: !flag("plain"), image: if flag("image") { Some(&mut **image) } else { None } },
        )
        .expect("nested too deep");
        ui::use_theme("dark");
        let lines = res.lines.iter().map(|l| {
            let segs = l.segs.iter().map(|(t, s)| Value::Arr(vec![Value::Str(t.clone()), Value::Str(s.clone())]));
            let figure = l
                .figure
                .as_ref()
                .and_then(|f| f.downcast_ref::<(String, usize)>())
                .map_or(Value::Null, |(u, r)| Value::Arr(vec![Value::Str(u.clone()), Value::Num(*r as f64)]));
            herdr_explorer::obj! {
                "segs" => Value::Arr(segs.collect()),
                "anchor" => l.anchor.clone().map_or(Value::Null, Value::Str),
                "figure" => figure,
            }
        });
        let anchors = res.anchors.into_iter().map(|(a, k)| (a, Value::Num(k as f64))).collect();
        Ok(herdr_explorer::obj! { "lines" => Value::Arr(lines.collect()), "anchors" => Value::Obj(anchors) })
    });
}

/// What a folder holds, as the fixture's tree(): each path, a folder ending with /, a file with its text, a link with
/// its target.
#[cfg(unix)]
fn tree(dir: &str) -> Vec<String> {
    fn walk(d: &std::path::Path, rel: &str, out: &mut Vec<String>) {
        let mut names: Vec<String> =
            std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for n in names {
            let p = d.join(&n);
            let r = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.file_type().is_symlink() {
                out.push(format!("{r} -> {}", std::fs::read_link(&p).unwrap().display()));
            } else if m.is_dir() {
                out.push(format!("{r}/"));
                walk(&p, &r, out);
            } else {
                out.push(format!("{r}: {}", std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new(dir), "", &mut out);
    out
}

#[cfg(unix)]
#[test]
fn core_actions() {
    use herdr_explorer::core::actions;
    const FAKES: &str = r#"mkdir -p ../bin ../trash
printf '#!/bin/sh\n[ -e "%s/no-trash" ] && exit 1\n[ "$1" = trash ] && exec /bin/mv "$3" "%s/trash/"\nexit 1\n' "$PWD/.." "$PWD/.." > ../bin/gio
printf '#!/bin/sh\nexit 1\n' > ../bin/trash-put
chmod +x ../bin/gio ../bin/trash-put
git init -q -b main . && git config user.name t && git config user.email t@t && git config commit.gpgsign false
"#;
    support::replay("core/actions", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        let opt = |v: Option<String>| v.map_or(Value::Null, Value::Str);
        let made = |(p, e): actions::Made| Value::Arr(vec![opt(p), opt(e)]);
        // The call's result, then what the folder holds; '@' in the path stands for the folder.
        let after = |v: Value| Value::Arr(vec![v, str_arr(tree(&s(0)))]);
        let at = |i: usize| s(i).replace('@', &s(0));
        Ok(match name {
            "quote" => Value::Str(actions::quote(&s(0))),
            "target" => opt(actions::target(&s(0), &s(1))),
            "sideSplit" => actions::side_split(&args[0], &s(1)).map_or(Value::Null, |f| {
                herdr_explorer::obj! {
                    "width" => Value::Num(f.width),
                    "splitWidth" => Value::Num(f.split_width),
                    "ratio" => Value::Num(f.ratio),
                    "left" => Value::Bool(f.left),
                }
            }),
            "webPage" => Value::Bool(actions::web_page(&s(0))),
            "canPreview" => Value::Bool(actions::can_preview(&s(0))),
            "canOpen" => Value::Bool(actions::can_open(&s(0))),
            "relativeTo" => Value::Str(actions::relative_to(Some(&s(0)), &s(1))),
            "create" => after(made(actions::create(&s(1), &s(2), args[3].truthy()))),
            "rename" => after(made(actions::rename(&at(1), &s(2)))),
            "duplicate" => after(made(actions::duplicate(&at(1)))),
            "remove" => after(opt(actions::remove(&at(1)))),
            "git" => {
                let top = format!("{}/repo", s(1));
                std::fs::create_dir(&top).unwrap();
                sh(&format!("cd '{top}' && {FAKES}{}", s(2)));
                let file = format!("{top}/{}", s(3));
                let msg = match s(0).as_str() {
                    "stage" => actions::stage(&file, &top),
                    "unstage" => actions::unstage(&file, &top),
                    _ => actions::discard(&file, &top),
                };
                let status = sh(&format!("cd '{top}' && git status --porcelain=v1 --untracked-files=all"));
                let mut trash: Vec<String> = std::fs::read_dir(format!("{}/trash", s(1)))
                    .unwrap()
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                trash.sort();
                Value::Arr(vec![Value::Str(msg), Value::Str(status), str_arr(trash)])
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}

#[cfg(unix)]
#[test]
fn explorer_tree() {
    use herdr_explorer::explorer::tree::{self, Entry, Ordered, Settings, Tree};
    fn pairs(m: &Ordered<char>) -> Value {
        Value::Arr(m.iter().map(|(k, c)| Value::Arr(vec![Value::Str(k.into()), Value::Str(c.to_string())])).collect())
    }
    fn keys(m: &Ordered<()>) -> Value {
        Value::Arr(m.iter().map(|(k, _)| Value::Str(k.into())).collect())
    }
    fn row(r: &Entry) -> Value {
        let opt = |o: &Option<String>| o.clone().map_or(Value::Null, Value::Str);
        // The match as the JS gives it: offsets in UTF-16 units.
        let units = |b: usize| Value::Num(r.name[..b].encode_utf16().count() as f64);
        let hit = r.hit.map_or(Value::Null, |(a, b)| Value::Arr(vec![units(a), units(b)]));
        herdr_explorer::obj! {
            "name" => Value::Str(r.name.clone()), "dir" => Value::Bool(r.dir), "rel" => Value::Str(r.rel.clone()),
            "link" => Value::Bool(r.link), "gone" => Value::Bool(r.gone), "from" => opt(&r.from),
            "chain" => opt(&r.chain), "depth" => Value::Num(r.depth as f64), "hit" => hit, "open" => Value::Bool(r.open)
        }
    }
    fn entry(v: &Value) -> Entry {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        Entry {
            name: s("name").unwrap_or_default(),
            dir: v.get("dir").and_then(Value::as_bool).unwrap_or(false),
            rel: s("rel").unwrap_or_default(),
            from: s("from"),
            ..Entry::default()
        }
    }
    support::replay("explorer/tree", |name, args| {
        let s = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
        let b = |i: usize| args.get(i).and_then(Value::as_bool).unwrap_or(false);
        let ch = |i: usize| s(i).chars().next().unwrap_or('\0');
        Ok(match name {
            "code" => Value::Str(tree::code(ch(0), ch(1)).to_string()),
            "parseStatus" => {
                let git = tree::parse_status(&s(0), &s(1));
                herdr_explorer::obj! {
                    "files" => pairs(&git.files), "dirs" => pairs(&git.dirs),
                    "untracked" => keys(&git.untracked), "ignored" => keys(&git.ignored)
                }
            }
            "treeBefore" => Value::Bool(tree::tree_before(&s(0), b(1), &s(2), b(3))),
            "parentOf" => tree::parent_of(&s(0)).map_or(Value::Null, |p| Value::Str(p.into())),
            "holds" => Value::Bool(tree::holds(&entry(&args[0]), &s(1))),
            "entryOrder" => {
                let mut list: Vec<Entry> = args[0].as_array().unwrap().iter().map(entry).collect();
                list.sort_by(tree::entry_order);
                str_arr(list.into_iter().map(|e| e.name).collect())
            }
            "rows" => {
                let flag = |k: &str| args[1].get(k).and_then(Value::as_bool).unwrap_or(false);
                let settings = Settings { hidden: flag("hidden"), ignored: flag("ignored"), compact: flag("compact") };
                let mut tree = Tree::new(&s(0), settings);
                tree.apply_status(&s(2));
                let view = &args[3];
                let query = view.get("query").and_then(Value::as_str);
                let changed = view.get("changed").and_then(Value::as_bool).unwrap_or(false);
                let expanded = strs(view.get("expanded").unwrap_or(&Value::Null));
                Value::Arr(tree.rows(query, changed, &expanded).iter().map(row).collect())
            }
            "changedFiles" => {
                let mut tree = Tree::new(&s(0), Settings { hidden: false, ignored: true, compact: false });
                tree.apply_status(&s(1));
                str_arr(tree.changed_files())
            }
            _ => return Err(format!("no function {name}")),
        })
    });
}
