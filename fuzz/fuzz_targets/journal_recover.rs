//! Any segment bytes after a valid header must be read, truncated as a torn tail, or refused
//! as corruption; never a panic. A read that succeeds must be repeatable, and after repair the
//! journal must read cleanly with the same records.
#![no_main]
use brier_journal::{JournalError, read_journal, recover};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let dir = tempfile::tempdir().unwrap();
    let mut seg = Vec::new();
    seg.extend_from_slice(b"BRIERJNL");
    seg.extend_from_slice(&2u16.to_le_bytes());
    seg.extend_from_slice(&1u64.to_le_bytes());
    seg.extend_from_slice(data);
    std::fs::write(dir.path().join("journal-00000000000000000001.log"), seg).unwrap();
    let mut first = Vec::new();
    let read = read_journal::<JournalError>(dir.path(), |s, c| {
        first.push((s, c));
        Ok(())
    });
    if read.is_ok() {
        let mut repaired = Vec::new();
        recover::<JournalError>(dir.path(), |s, c| {
            repaired.push((s, c));
            Ok(())
        })
        .unwrap();
        assert_eq!(first, repaired, "repair changed the intact records");
        let mut again = Vec::new();
        let clean = read_journal::<JournalError>(dir.path(), |s, c| {
            again.push((s, c));
            Ok(())
        })
        .unwrap();
        assert!(clean.truncation.is_none(), "a repaired journal still has a torn tail");
        assert_eq!(first, again);
    }
});
