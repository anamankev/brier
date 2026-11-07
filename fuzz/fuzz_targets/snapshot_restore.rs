//! Any bytes must restore to a state that passes every invariant, or be refused; never a
//! panic. A restored state must snapshot back to exactly the same bytes when they are in the
//! current format (the encoding is canonical).
#![no_main]
use brier_engine::{Engine, SNAPSHOT_BODY_VERSION};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(engine) = Engine::restore(data) {
        engine.check_invariants().unwrap();
        if data.first() == Some(&SNAPSHOT_BODY_VERSION) {
            assert_eq!(engine.snapshot().unwrap(), data, "snapshot encoding is not canonical");
        }
    }
});
