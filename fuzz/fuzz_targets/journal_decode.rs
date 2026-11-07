//! Any payload must decode or be refused, and a payload that decodes in the current format
//! must be the canonical encoding of its command (one command, one encoding: ADR-0006).
#![no_main]
use brier_journal::{FORMAT_VERSION, decode, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode(data, 1);
    if let Ok(command) = decode(data, FORMAT_VERSION) {
        let mut again = Vec::new();
        encode(&command, &mut again);
        assert_eq!(again, data, "decoding is not canonical");
    }
});
