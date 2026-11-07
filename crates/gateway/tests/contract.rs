//! The API documents are held to the code (SPEC §6: every endpoint, message, and error code is
//! documented): `docs/api/openapi.yaml` lists exactly the endpoints the router serves and
//! exactly the error codes the gateway and engine produce, and `docs/api/asyncapi.yaml` lists
//! exactly the WebSocket client ops and server message types. Runs in `make check`; no
//! database needed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)] // test code

use std::collections::BTreeSet;
use std::path::PathBuf;

use brier_engine::RejectReason;
use brier_gateway::ENDPOINTS;
use brier_gateway::error::GATEWAY_CODES;
use brier_gateway::ws::{OPS, SERVER_MESSAGES};

fn doc(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/api")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// "METHOD path" for every operation under `paths:` (two-space path keys, four-space methods).
fn openapi_endpoints(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut path = None;
    let mut in_paths = false;
    for line in text.lines() {
        if !line.starts_with(' ') {
            in_paths = line == "paths:";
            continue;
        }
        if !in_paths {
            continue;
        }
        if let Some(p) = line.strip_prefix("  /").and_then(|l| l.strip_suffix(':')) {
            path = Some(format!("/{p}"));
        } else if let Some(m) = line.strip_prefix("    ").and_then(|l| l.strip_suffix(':'))
            && ["get", "post", "put", "patch", "delete"].contains(&m)
        {
            out.insert(format!(
                "{} {}",
                m.to_uppercase(),
                path.clone().expect("a method under a path")
            ));
        }
    }
    out
}

/// The items of the block list right after the line `ErrorCode:` ... `enum:`.
fn openapi_error_codes(text: &str) -> BTreeSet<String> {
    let after = text
        .split_once("    ErrorCode:")
        .expect("an ErrorCode schema")
        .1;
    let list = after.split_once("      enum:\n").expect("an enum").1;
    list.lines()
        .map_while(|l| l.strip_prefix("        - "))
        .map(str::to_owned)
        .collect()
}

/// Every `<field>: { const: <value> }` in the document.
fn consts(text: &str, field: &str) -> BTreeSet<String> {
    let needle = format!("{field}: {{ const: ");
    text.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix(&needle)
                .and_then(|r| r.strip_suffix(" }"))
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn openapi_lists_exactly_the_served_endpoints() {
    let documented = openapi_endpoints(&doc("openapi.yaml"));
    let served: BTreeSet<String> = ENDPOINTS.iter().map(|(m, p)| format!("{m} {p}")).collect();
    assert_eq!(documented, served);
    assert_eq!(
        served.len(),
        ENDPOINTS.len(),
        "no endpoint is declared twice"
    );
}

#[test]
fn openapi_lists_exactly_the_error_codes() {
    let documented = openapi_error_codes(&doc("openapi.yaml"));
    let mut produced: BTreeSet<String> = GATEWAY_CODES.iter().map(|&c| c.to_owned()).collect();
    produced.extend(
        RejectReason::ALL
            .iter()
            .map(|&r| brier_wire::reject_name(r).to_owned()),
    );
    assert_eq!(documented, produced);
}

#[test]
fn asyncapi_lists_exactly_the_ops_and_message_types() {
    let text = doc("asyncapi.yaml");
    let ops: BTreeSet<String> = OPS.iter().map(|&o| o.to_owned()).collect();
    assert_eq!(consts(&text, "op"), ops);
    let types: BTreeSet<String> = SERVER_MESSAGES.iter().map(|&t| t.to_owned()).collect();
    assert_eq!(consts(&text, "type"), types);
}
