//! The shared rulebook vectors (`tests/vectors.json`, written by `scripts/rulebook_vectors.py`
//! with Python's own JSON encoder): every expected error code, canonical form, and hash. The
//! Python twin runs the same file, so the two validators cannot drift apart.
#![allow(clippy::unwrap_used)] // a failure must stop the test

use brier_rules::{canonical::canonical, validate};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

fn vectors() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors.json"))
        .unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn every_vector_matches() {
    let vectors = vectors();
    assert!(vectors.len() >= 39);
    for v in vectors {
        let name = v["name"].as_str().unwrap();
        match (validate(&v["input"]), v["error"].as_str()) {
            (Ok(ok), None) => {
                assert_eq!(ok.canonical, v["canonical"].as_str().unwrap(), "{name}");
                assert_eq!(ok.hash_hex(), v["sha256"].as_str().unwrap(), "{name}");
                assert!(ok.describe().starts_with("Resolves YES if "));
            }
            (Err(e), Some(code)) => assert_eq!(e.code(), code, "{name}: {e}"),
            (got, want) => panic!("{name}: got {got:?}, want {want:?}"),
        }
    }
}

#[test]
fn describe_states_the_condition() {
    let cpi = vectors()
        .into_iter()
        .find(|v| v["name"] == "cpi_valid")
        .unwrap();
    let text = validate(&cpi["input"]).unwrap().describe();
    assert_eq!(
        text,
        "Resolves YES if the 12-month percent change in CPI-U, U.S. city average, all items, not \
         seasonally adjusted, rounded to 1 decimal place, for the month 2026-09, as first \
         published by the source, is above 3.0 percent. Resolves NO if it is published and is \
         not. Voided if no value is published by 2026-12-15T12:30:00Z, or if the source \
         discontinues or redefines the series before publishing it."
    );
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (-(1i64 << 53) + 1..(1i64 << 53)).prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::vec((any::<String>(), inner), 0..6)
                .prop_map(|kv| Value::Object(kv.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

fn reversed(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<_> = m.keys().cloned().collect();
            keys.reverse();
            Value::Object(
                keys.into_iter()
                    .map(|k| {
                        let x = reversed(&m[&k]);
                        (k, x)
                    })
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(reversed).collect()),
        other => other.clone(),
    }
}

proptest! {
    /// Canonical text parses back to the same value, and canonicalizing it again changes
    /// nothing; key order in the input never matters.
    #[test]
    fn canonical_is_a_fixed_point_and_order_free(v in json_value()) {
        let c = canonical(&v).unwrap();
        let back: Value = serde_json::from_str(&c).unwrap();
        prop_assert_eq!(&back, &v);
        prop_assert_eq!(canonical(&back).unwrap(), c.clone());
        prop_assert_eq!(canonical(&reversed(&v)).unwrap(), c);
    }
}

/// `registry.json` is the registry as data; the Python twin is checked against the same file.
#[test]
fn registry_file_matches_the_code() {
    use brier_rules::{REGISTRY, time::PeriodKind};
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/registry.json")).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let code: Vec<Value> = REGISTRY
        .iter()
        .map(|s| {
            json!({
                "provider": serde_json::to_value(s.provider).unwrap(),
                "id": s.id,
                "name": s.name,
                "url": s.url,
                "kind": match s.kind { PeriodKind::Month => "month", PeriodKind::Day => "day" },
                "measures": s.measures.iter().map(|m| json!({
                    "transform": serde_json::to_value(m.transform).unwrap(),
                    "precision": m.precision,
                    "unit": m.unit,
                })).collect::<Vec<_>>(),
                "category": s.category,
                "live": s.live,
                "released_after_period": s.released_after_period,
                "machine_checkable": s.machine_checkable,
            })
        })
        .collect();
    assert_eq!(file, Value::Array(code));
}

#[test]
fn describe_states_a_range_with_its_inclusive_ends() {
    let bracket = vectors()
        .into_iter()
        .find(|v| v["name"] == "v2_bracket_valid")
        .unwrap();
    let text = validate(&bracket["input"]).unwrap().describe();
    assert!(
        text.contains("is between 88 and 89 (inclusive), °F."),
        "{text}"
    );
}
