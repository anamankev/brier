//! Signed-request verification must never panic on any headers, key, or signature, and must
//! be exact: a signature by the right key over the right request verifies, and changing any
//! signed part (timestamp, method, path, body) or the key makes it fail.
#![no_main]
use brier_gateway::auth::{SignedHeaders, signed_headers, signing_payload, verify_signature};
use ed25519_dalek::{Signer, SigningKey};
use http::{HeaderMap, HeaderValue, Method};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Arbitrary headers: three values split from the input.
    let mut parts = data.splitn(4, |&b| b == b'\n');
    let (key, ts, sig, rest) = (parts.next(), parts.next(), parts.next(), parts.next().unwrap_or_default());
    let mut headers = HeaderMap::new();
    for (name, value) in [("brier-key", key), ("brier-timestamp", ts), ("brier-signature", sig)] {
        if let Some(v) = value.and_then(|v| HeaderValue::from_bytes(v).ok()) {
            headers.insert(name, v);
        }
    }
    let parsed = signed_headers(&headers);
    if let (Ok(Some(signed)), Some(public)) = (parsed, rest.get(..32)) {
        let _ = verify_signature(public, &signed, &Method::POST, "/v1/orders", rest);
    }

    // A real signature over a fuzz-chosen request.
    let Some((&seed, body)) = rest.split_first() else { return };
    let key = SigningKey::from_bytes(&[seed; 32]);
    let public = key.verifying_key().to_bytes();
    let timestamp = u64::from(seed).wrapping_mul(1_000_003);
    let path = "/v1/orders?after_seq=1";
    let signature = key.sign(&signing_payload(timestamp, &Method::POST, path, body)).to_bytes();
    let signed = SignedHeaders { key_id: 1, timestamp, signature };
    assert!(verify_signature(&public, &signed, &Method::POST, path, body).is_ok());
    let later = SignedHeaders { timestamp: timestamp.wrapping_add(1), ..signed };
    assert!(verify_signature(&public, &later, &Method::POST, path, body).is_err());
    assert!(verify_signature(&public, &signed, &Method::PATCH, path, body).is_err());
    assert!(verify_signature(&public, &signed, &Method::POST, "/v1/orders?after_seq=2", body).is_err());
    let mut changed = body.to_vec();
    changed.push(0);
    assert!(verify_signature(&public, &signed, &Method::POST, path, &changed).is_err());
    let other = SigningKey::from_bytes(&[seed.wrapping_add(1); 32]).verifying_key().to_bytes();
    assert!(verify_signature(&other, &signed, &Method::POST, path, body).is_err());
});
