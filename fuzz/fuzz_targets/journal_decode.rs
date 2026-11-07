//! Any payload must decode or be refused in every readable format version. A payload that
//! decodes in the current format must be the canonical encoding of its command (one command,
//! one encoding: ADR-0006), and a command read from an older format must keep its meaning when
//! written in the current one.
#![no_main]
use brier_journal::{FORMAT_VERSION, decode, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for version in 1..FORMAT_VERSION {
        if let Ok(command) = decode(data, version) {
            let mut current = Vec::new();
            encode(&command, &mut current);
            assert_eq!(
                decode(&current, FORMAT_VERSION),
                Ok(command),
                "an older record changes meaning when rewritten"
            );
        }
    }
    if let Ok(command) = decode(data, FORMAT_VERSION) {
        let mut again = Vec::new();
        encode(&command, &mut again);
        assert_eq!(again, data, "decoding is not canonical");
    }
});
