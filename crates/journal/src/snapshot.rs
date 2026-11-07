//! Snapshot files: written atomically, read with full checks (ADR-0006).
//!
//! A snapshot at sequence S holds the engine state after command S. Recovery loads the
//! newest valid snapshot and replays the journal after S. The journal keeps every command from
//! sequence 1, so a damaged snapshot is never fatal: recovery falls back to an older one, or to
//! a replay from empty, and reports what it skipped.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use brier_types::Seq;

use crate::crc::crc32c;
use crate::log::JournalError;

const MAGIC: &[u8; 8] = b"BRIERSNP";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 8 + 2 + 8 + 32 + 8 + 4;
/// How many snapshots to keep; older ones are deleted after a new one is durable.
pub const KEEP_SNAPSHOTS: usize = 2;

/// A snapshot read back from disk, with its header checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    /// The last command the snapshot includes.
    pub seq: Seq,
    /// The engine's state hash at that point, as recorded when the snapshot was taken.
    pub state_hash: [u8; 32],
    /// The engine snapshot body (`Engine::restore` input).
    pub body: Vec<u8>,
}

/// The newest valid snapshot, and any newer files that were skipped as invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestSnapshot {
    /// The snapshot to restore, if any valid one exists.
    pub snapshot: Option<SnapshotFile>,
    /// Snapshot files that failed their checks, newest first, with the reason.
    pub skipped: Vec<(PathBuf, &'static str)>,
}

fn snapshot_path(dir: &Path, seq: Seq) -> PathBuf {
    dir.join(format!("snapshot-{:020}.bin", seq.get()))
}

fn sync_dir(dir: &Path) -> Result<(), JournalError> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// Snapshot files in `dir`, newest first. Leftover temporary files from a crash mid-write are
/// deleted: they were never renamed into place, so nothing relied on them.
fn snapshot_files(dir: &Path) -> Result<Vec<(u64, PathBuf)>, JournalError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("snapshot-") && name.ends_with(".bin.tmp") {
            fs::remove_file(&path)?;
            continue;
        }
        let Some(number) = name
            .strip_prefix("snapshot-")
            .and_then(|n| n.strip_suffix(".bin"))
        else {
            continue;
        };
        if let Ok(seq) = number.parse::<u64>() {
            found.push((seq, path));
        }
    }
    found.sort_unstable_by(|a, b| b.cmp(a));
    Ok(found)
}

/// Write a snapshot of the state after command `seq` atomically: temporary file, sync,
/// rename, sync the directory. Then delete all but the newest [`KEEP_SNAPSHOTS`].
///
/// # Errors
/// [`JournalError::Io`] if any step fails; the previous snapshots are then untouched.
pub fn write_snapshot(
    dir: &Path,
    seq: Seq,
    state_hash: &[u8; 32],
    body: &[u8],
) -> Result<PathBuf, JournalError> {
    let path = snapshot_path(dir, seq);
    let tmp = path.with_extension("bin.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)?;
    file.write_all(MAGIC)?;
    file.write_all(&VERSION.to_le_bytes())?;
    file.write_all(&seq.get().to_le_bytes())?;
    file.write_all(state_hash)?;
    file.write_all(&(body.len() as u64).to_le_bytes())?;
    file.write_all(&crc32c(body).to_le_bytes())?;
    file.write_all(body)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, &path)?;
    sync_dir(dir)?;
    for (_, old) in snapshot_files(dir)?.into_iter().skip(KEEP_SNAPSHOTS) {
        fs::remove_file(old)?;
    }
    Ok(path)
}

fn read_snapshot(path: &Path, named_seq: u64) -> Result<SnapshotFile, &'static str> {
    let bytes = fs::read(path).map_err(|_| "unreadable")?;
    let (header, body) = bytes.split_at_checked(HEADER_LEN).ok_or("short header")?;
    let field = |from: usize, to: usize| header.get(from..to).ok_or("short header");
    if field(0, 8)? != MAGIC.as_slice() {
        return Err("bad magic");
    }
    if field(8, 10)? != VERSION.to_le_bytes().as_slice() {
        return Err("unsupported version");
    }
    let seq = u64::from_le_bytes(field(10, 18)?.try_into().map_err(|_| "seq field")?);
    let state_hash: [u8; 32] = field(18, 50)?.try_into().map_err(|_| "hash field")?;
    let len = u64::from_le_bytes(field(50, 58)?.try_into().map_err(|_| "length field")?);
    let crc = u32::from_le_bytes(field(58, 62)?.try_into().map_err(|_| "crc field")?);
    if seq != named_seq || seq == 0 {
        return Err("sequence number does not match the file name");
    }
    if body.len() as u64 != len {
        return Err("body length does not match the header");
    }
    if crc32c(body) != crc {
        return Err("body checksum mismatch");
    }
    Ok(SnapshotFile {
        seq: Seq::from_raw(seq),
        state_hash,
        body: body.to_vec(),
    })
}

/// The newest snapshot in `dir` that passes every check, skipping (and reporting) damaged ones.
///
/// # Errors
/// [`JournalError::Io`] if the directory cannot be read.
pub fn latest_snapshot(dir: &Path) -> Result<LatestSnapshot, JournalError> {
    let mut skipped = Vec::new();
    for (seq, path) in snapshot_files(dir)? {
        match read_snapshot(&path, seq) {
            Ok(snapshot) => {
                return Ok(LatestSnapshot {
                    snapshot: Some(snapshot),
                    skipped,
                });
            }
            Err(reason) => skipped.push((path, reason)),
        }
    }
    Ok(LatestSnapshot {
        snapshot: None,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_snapshot_reads_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"engine state bytes".to_vec();
        write_snapshot(dir.path(), Seq::from_raw(42), &[7; 32], &body).unwrap();
        let latest = latest_snapshot(dir.path()).unwrap();
        assert_eq!(
            latest.snapshot,
            Some(SnapshotFile {
                seq: Seq::from_raw(42),
                state_hash: [7; 32],
                body
            })
        );
        assert_eq!(latest.skipped, vec![]);
    }

    #[test]
    fn only_the_newest_snapshots_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for seq in 1..=5 {
            write_snapshot(dir.path(), Seq::from_raw(seq), &[0; 32], b"x").unwrap();
        }
        let left: Vec<u64> = snapshot_files(dir.path())
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(left, vec![5, 4]);
    }

    #[test]
    fn a_damaged_newest_snapshot_falls_back_to_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(dir.path(), Seq::from_raw(10), &[1; 32], b"older").unwrap();
        let newest = write_snapshot(dir.path(), Seq::from_raw(20), &[2; 32], b"newer").unwrap();
        let mut bytes = fs::read(&newest).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&newest, bytes).unwrap();
        let latest = latest_snapshot(dir.path()).unwrap();
        assert_eq!(latest.snapshot.map(|s| s.seq), Some(Seq::from_raw(10)));
        assert_eq!(latest.skipped, vec![(newest, "body checksum mismatch")]);
    }

    #[test]
    fn a_leftover_temporary_file_is_removed_and_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("snapshot-00000000000000000009.bin.tmp");
        fs::write(&tmp, b"half written").unwrap();
        assert_eq!(latest_snapshot(dir.path()).unwrap().snapshot, None);
        assert!(!tmp.exists());
    }
}
