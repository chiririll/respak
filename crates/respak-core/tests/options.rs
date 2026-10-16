//! `TypedTarget`: option parsing, defaults and normalization.

use respak_core::{ErrorKind, FormatId, Target, TypedTarget, ValueKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Opts {
    level: u32,
    mode: String,
    gamma: Option<f32>,
}

fn target(options: serde_json::Value) -> Target {
    Target {
        format: FormatId::new("x"),
        options,
        via: vec![],
    }
}

#[test]
fn missing_fields_come_from_default_even_without_serde_default() {
    let t = TypedTarget::<Opts>::parse(&target(json!({ "level": 3 }))).unwrap();
    assert_eq!(
        t.options(),
        &Opts {
            level: 3,
            mode: String::new(),
            gamma: None
        }
    );
    let t = TypedTarget::<Opts>::parse(&target(serde_json::Value::Null)).unwrap();
    assert_eq!(t.options(), &Opts::default());
}

#[test]
fn bad_options_are_invalid_options() {
    for bad in [
        json!({ "level": "high" }),
        json!({ "nope": 1 }),
        json!([1, 2]),
        json!(5),
    ] {
        let err = TypedTarget::<Opts>::parse(&target(bad.clone())).unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::InvalidOptions { .. }),
            "{bad}: {err}"
        );
    }
}

#[test]
fn normalization_fills_defaults_and_sorts_so_equal_requests_give_equal_keys() {
    let key = |options| {
        let t = TypedTarget::<Opts>::parse(&target(options)).unwrap();
        ValueKey::converted(
            Arc::new(ValueKey::file("a.bin")),
            FormatId::new("x"),
            t.normalized(),
        )
    };
    let a = key(json!({}));
    let b = key(json!({ "mode": "", "level": 0, "gamma": null }));
    let c = key(json!({ "level": 1 }));
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        a.to_string(),
        r#"a.bin => x {"gamma":null,"level":0,"mode":""}"#
    );
}
