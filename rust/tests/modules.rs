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

#[cfg(unix)]
#[test]
#[ignore = "case 5"]
fn core_exec() {
    support::replay("core/exec", |_, _| Err("not ported".into()));
}

#[cfg(unix)]
#[test]
#[ignore = "case 5"]
fn core_places() {
    support::replay("core/places", |_, _| Err("not ported".into()));
}

#[cfg(unix)]
#[test]
#[ignore = "case 5"]
fn core_scope() {
    support::replay("core/scope", |_, _| Err("not ported".into()));
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
