//! Any line a client sends must parse or be refused; it must never panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(line) = std::str::from_utf8(data) {
        let _ = brier_wire::parse_command(line);
    }
});
