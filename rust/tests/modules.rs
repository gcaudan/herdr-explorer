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
        assert_eq!(
            format!("{}\n", json::stringify_pretty(&v, 1)),
            text,
            "{} written back",
            path.display()
        );
        for case in v.get("cases").and_then(Value::as_array).unwrap() {
            assert!(
                case.get("fn").and_then(Value::as_str).is_some(),
                "{}: fn",
                path.display()
            );
            assert!(
                case.get("args").and_then(Value::as_array).is_some(),
                "{}: args",
                path.display()
            );
            assert!(
                case.get("out").is_some() || case.get("throws").is_some(),
                "{}: outcome",
                path.display()
            );
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
