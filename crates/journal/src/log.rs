//! Journal segments: writing with group commit, and recovery with torn-tail handling
//! (ADR-0004, ADR-0006).

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use brier_engine::Command;
use brier_types::Seq;

use crate::codec::{decode, encode};
use crate::crc::Crc32c;

/// A segment is rotated once it reaches this size.
pub const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
/// Largest group-commit batch. A torn tail can be at most one batch long (ADR-0006).
pub const MAX_BATCH_BYTES: usize = 1024 * 1024;
/// Largest encoded command; any longer length field is damage, not data.
pub const MAX_PAYLOAD: usize = 64;

const MAGIC: &[u8; 8] = b"BRIERJNL";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 8 + 2 + 8;
const RECORD_HEADER_LEN: usize = 4 + 4 + 8;

/// A journal failure. Corruption means data that may have been acknowledged is damaged:
/// recovery stops rather than guess.
#[derive(Debug)]
pub enum JournalError {
    /// The file system failed.
    Io(io::Error),
    /// A segment holds damaged or inconsistent data that cannot be a torn tail.
    Corrupt {
        /// The segment file.
        segment: PathBuf,
        /// Byte offset of the damage within it.
        offset: u64,
        /// What is wrong.
        reason: &'static str,
    },
    /// The writer was handed a command out of sequence.
    OutOfSequence {
        /// The sequence number expected next.
        expected: Seq,
        /// The one given.
        got: Seq,
    },
    /// Appending would make the pending batch larger than `MAX_BATCH_BYTES`; commit first.
    BatchFull,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "journal i/o error: {e}"),
            Self::Corrupt {
                segment,
                offset,
                reason,
            } => {
                write!(
                    f,
                    "journal corrupt in {} at byte {offset}: {reason}",
                    segment.display()
                )
            }
            Self::OutOfSequence { expected, got } => {
                write!(
                    f,
                    "journal append out of sequence: expected {}, got {}",
                    expected.get(),
                    got.get()
                )
            }
            Self::BatchFull => f.write_str("journal batch is full; commit before appending more"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<io::Error> for JournalError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A torn tail found and removed during recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    /// The segment that was cut.
    pub segment: PathBuf,
    /// Where it was cut: the start of the first bad record (or 0 for a torn header).
    pub offset: u64,
    /// How many bytes were dropped.
    pub dropped: u64,
}

/// What recovery found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// The sequence number of the last intact record, if any.
    pub last_seq: Option<Seq>,
    /// The torn tail that was removed, if there was one.
    pub truncation: Option<Truncation>,
}

fn segment_path(dir: &Path, first: Seq) -> PathBuf {
    dir.join(format!("journal-{:020}.log", first.get()))
}

/// Segment files in `dir`, sorted by their first sequence number.
fn segments(dir: &Path) -> Result<Vec<(u64, PathBuf)>, JournalError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(number) = name
            .strip_prefix("journal-")
            .and_then(|n| n.strip_suffix(".log"))
        else {
            continue;
        };
        let first: u64 = number.parse().map_err(|_| JournalError::Corrupt {
            segment: path.clone(),
            offset: 0,
            reason: "segment file name is not journal-<sequence>.log",
        })?;
        found.push((first, path));
    }
    found.sort_unstable();
    Ok(found)
}

fn sync_dir(dir: &Path) -> Result<(), JournalError> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn header(first: Seq) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&VERSION.to_le_bytes());
    h.extend_from_slice(&first.get().to_le_bytes());
    h
}

fn record_checksum(seq: u64, payload: &[u8]) -> u32 {
    Crc32c::new()
        .update(&seq.to_le_bytes())
        .update(payload)
        .finish()
}

/// Why one record could not be read.
enum BadRecord {
    /// Damage of the kind a torn write leaves: short, oversized, or a checksum mismatch.
    Torn(&'static str),
    /// A well-formed record that contradicts the log: wrong sequence or undecodable.
    Inconsistent(&'static str),
}

/// Read one record at the start of `bytes`: its total length, sequence, and command.
fn read_record(bytes: &[u8], expected: u64) -> Result<(usize, u64, Command), BadRecord> {
    let (head, rest) = bytes
        .split_at_checked(RECORD_HEADER_LEN)
        .ok_or(BadRecord::Torn("short record header"))?;
    let field = |range: std::ops::Range<usize>| {
        head.get(range)
            .ok_or(BadRecord::Torn("short record header"))
    };
    let len = u32::from_le_bytes(
        field(0..4)?
            .try_into()
            .map_err(|_| BadRecord::Torn("length field"))?,
    );
    let crc = u32::from_le_bytes(
        field(4..8)?
            .try_into()
            .map_err(|_| BadRecord::Torn("crc field"))?,
    );
    let seq = u64::from_le_bytes(
        field(8..16)?
            .try_into()
            .map_err(|_| BadRecord::Torn("seq field"))?,
    );
    let len = usize::try_from(len).map_err(|_| BadRecord::Torn("length field"))?;
    if len == 0 || len > MAX_PAYLOAD {
        return Err(BadRecord::Torn("record length out of range"));
    }
    let payload = rest.get(..len).ok_or(BadRecord::Torn("short payload"))?;
    if record_checksum(seq, payload) != crc {
        return Err(BadRecord::Torn("checksum mismatch"));
    }
    if seq != expected {
        return Err(BadRecord::Inconsistent(
            "sequence number is not the next one",
        ));
    }
    let command =
        decode(payload).map_err(|_| BadRecord::Inconsistent("payload does not decode"))?;
    let total = RECORD_HEADER_LEN
        .checked_add(len)
        .ok_or(BadRecord::Torn("record length"))?;
    Ok((total, seq, command))
}

/// Read every intact record in `dir` in order, handing each to `apply`, and remove a torn
/// tail if there is one (ADR-0006). Stops with an error on corruption, or if `apply` fails.
///
/// # Errors
/// [`JournalError::Corrupt`] for damage that cannot be a torn tail, [`JournalError::Io`] for
/// file-system failures, or whatever `apply` returns, converted.
pub fn recover<E>(
    dir: &Path,
    mut apply: impl FnMut(Seq, Command) -> Result<(), E>,
) -> Result<Recovered, E>
where
    E: From<JournalError>,
{
    let files = segments(dir)?;
    let mut expected: u64 = Seq::FIRST.get();
    let mut last_seq = None;
    let last_index = files.len().checked_sub(1);
    for (index, (first, path)) in files.iter().enumerate() {
        let is_last = Some(index) == last_index;
        let corrupt = |offset: usize, reason| JournalError::Corrupt {
            segment: path.clone(),
            offset: offset as u64,
            reason,
        };
        let bytes = fs::read(path).map_err(JournalError::from)?;
        if bytes.len() < HEADER_LEN {
            if is_last {
                // A crash while creating the segment: nothing in it was ever acknowledged.
                fs::remove_file(path).map_err(JournalError::from)?;
                sync_dir(dir)?;
                let truncation = Truncation {
                    segment: path.clone(),
                    offset: 0,
                    dropped: bytes.len() as u64,
                };
                return Ok(Recovered {
                    last_seq,
                    truncation: Some(truncation),
                });
            }
            return Err(corrupt(0, "short segment header").into());
        }
        if bytes.get(..8) != Some(MAGIC.as_slice()) {
            return Err(corrupt(0, "bad segment magic").into());
        }
        if bytes.get(8..10) != Some(VERSION.to_le_bytes().as_slice()) {
            return Err(corrupt(8, "unsupported segment version").into());
        }
        let header_first = bytes
            .get(10..18)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes);
        if header_first != Some(*first) || *first != expected {
            return Err(corrupt(10, "segment does not start at the next sequence number").into());
        }
        let mut offset = HEADER_LEN;
        while let Some(rest) = bytes.get(offset..).filter(|r| !r.is_empty()) {
            match read_record(rest, expected) {
                Ok((total, seq, command)) => {
                    apply(Seq::from_raw(seq), command)?;
                    last_seq = Some(Seq::from_raw(seq));
                    expected = expected
                        .checked_add(1)
                        .ok_or_else(|| corrupt(offset, "sequence overflow"))?;
                    offset = offset
                        .checked_add(total)
                        .ok_or_else(|| corrupt(offset, "offset overflow"))?;
                }
                Err(BadRecord::Torn(reason)) => {
                    let dropped = rest.len();
                    if !is_last || dropped > MAX_BATCH_BYTES {
                        return Err(corrupt(offset, reason).into());
                    }
                    let file = OpenOptions::new()
                        .write(true)
                        .open(path)
                        .map_err(JournalError::from)?;
                    file.set_len(offset as u64).map_err(JournalError::from)?;
                    file.sync_all().map_err(JournalError::from)?;
                    let truncation = Truncation {
                        segment: path.clone(),
                        offset: offset as u64,
                        dropped: dropped as u64,
                    };
                    return Ok(Recovered {
                        last_seq,
                        truncation: Some(truncation),
                    });
                }
                Err(BadRecord::Inconsistent(reason)) => return Err(corrupt(offset, reason).into()),
            }
        }
    }
    Ok(Recovered {
        last_seq,
        truncation: None,
    })
}

/// Appends commands to the journal in batches, syncing once per batch (ADR-0004).
pub struct JournalWriter {
    dir: PathBuf,
    file: File,
    segment_len: u64,
    segment_limit: u64,
    next: Seq,
    batch: Vec<u8>,
    /// The sequence number of the first command in the pending batch.
    batch_first: Option<Seq>,
}

impl JournalWriter {
    /// Open the journal in `dir` for appending after recovery, with `next` the sequence
    /// number of the next command (one past [`Recovered::last_seq`]).
    ///
    /// # Errors
    /// [`JournalError::Io`] if the directory or segment cannot be opened or created.
    pub fn open(dir: &Path, next: Seq) -> Result<Self, JournalError> {
        Self::open_with_segment_limit(dir, next, SEGMENT_BYTES)
    }

    /// As [`JournalWriter::open`], with a custom segment size (tests use small segments).
    ///
    /// # Errors
    /// [`JournalError::Io`] if the directory or segment cannot be opened or created.
    pub fn open_with_segment_limit(
        dir: &Path,
        next: Seq,
        segment_limit: u64,
    ) -> Result<Self, JournalError> {
        fs::create_dir_all(dir)?;
        let (file, segment_len) = match segments(dir)?.pop() {
            Some((_, path)) => {
                let file = OpenOptions::new().append(true).open(&path)?;
                let len = file.metadata()?.len();
                (file, len)
            }
            None => Self::create_segment(dir, next)?,
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            file,
            segment_len,
            segment_limit,
            next,
            batch: Vec::new(),
            batch_first: None,
        })
    }

    fn create_segment(dir: &Path, first: Seq) -> Result<(File, u64), JournalError> {
        let path = segment_path(dir, first);
        let mut file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        let h = header(first);
        file.write_all(&h)?;
        file.sync_all()?;
        sync_dir(dir)?;
        Ok((file, h.len() as u64))
    }

    /// The sequence number the next appended command must carry.
    #[must_use]
    pub const fn next_seq(&self) -> Seq {
        self.next
    }

    /// Bytes waiting in the current batch.
    #[must_use]
    pub const fn pending_bytes(&self) -> usize {
        self.batch.len()
    }

    /// Add a command to the pending batch. Nothing is durable until [`JournalWriter::commit`].
    ///
    /// # Errors
    /// [`JournalError::OutOfSequence`] if `seq` is not the next sequence number, or
    /// [`JournalError::BatchFull`] if the batch would exceed `MAX_BATCH_BYTES`.
    pub fn append(&mut self, seq: Seq, command: &Command) -> Result<(), JournalError> {
        if seq != self.next {
            return Err(JournalError::OutOfSequence {
                expected: self.next,
                got: seq,
            });
        }
        let mut payload = Vec::with_capacity(MAX_PAYLOAD);
        encode(command, &mut payload);
        let record_len = RECORD_HEADER_LEN
            .checked_add(payload.len())
            .ok_or(JournalError::BatchFull)?;
        if self
            .batch
            .len()
            .checked_add(record_len)
            .is_none_or(|n| n > MAX_BATCH_BYTES)
        {
            return Err(JournalError::BatchFull);
        }
        let len = u32::try_from(payload.len()).map_err(|_| JournalError::BatchFull)?;
        self.batch.extend_from_slice(&len.to_le_bytes());
        self.batch
            .extend_from_slice(&record_checksum(seq.get(), &payload).to_le_bytes());
        self.batch.extend_from_slice(&seq.get().to_le_bytes());
        self.batch.extend_from_slice(&payload);
        self.batch_first.get_or_insert(seq);
        self.next = seq.next().map_err(|_| JournalError::OutOfSequence {
            expected: seq,
            got: seq,
        })?;
        Ok(())
    }

    /// Write the pending batch and sync it to the device. Every command appended before this
    /// call is durable once it returns `Ok` (ADR-0004), and may then be acknowledged.
    ///
    /// # Errors
    /// [`JournalError::Io`] if writing or syncing fails. The batch is then not durable, and the
    /// caller must not acknowledge it: the core halts and recovers from the journal.
    pub fn commit(&mut self) -> Result<(), JournalError> {
        if self.batch.is_empty() {
            return Ok(());
        }
        self.file.write_all(&self.batch)?;
        // On Apple targets `sync_data` issues fcntl(F_FULLFSYNC); see ADR-0004 and the
        // durability note for how this is verified.
        self.file.sync_data()?;
        self.segment_len = self.segment_len.saturating_add(self.batch.len() as u64);
        self.batch.clear();
        self.batch_first = None;
        if self.segment_len >= self.segment_limit {
            let (file, len) = Self::create_segment(&self.dir, self.next)?;
            self.file = file;
            self.segment_len = len;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)] // tests compute offsets and counts with plain arithmetic
mod tests {
    use super::*;
    use brier_types::{AccountId, MarketId, OrderId};
    use proptest::prelude::*;

    fn cmd(i: u64) -> Command {
        match i % 3 {
            0 => Command::OpenAccount {
                account: AccountId::new(i),
            },
            1 => Command::CreateMarket {
                market: MarketId::new(u32::try_from(i % 1000).unwrap()),
            },
            _ => Command::Cancel {
                account: AccountId::new(i),
                order: OrderId::new(i),
            },
        }
    }

    /// Write commands 1..=n in batches of `batch`, committing each batch.
    fn write(dir: &Path, n: u64, batch: u64, segment_limit: u64) {
        let mut w = JournalWriter::open_with_segment_limit(dir, Seq::FIRST, segment_limit).unwrap();
        for i in 1..=n {
            w.append(Seq::from_raw(i), &cmd(i)).unwrap();
            if i % batch == 0 {
                w.commit().unwrap();
            }
        }
        w.commit().unwrap();
    }

    fn read(dir: &Path) -> (Vec<(u64, Command)>, Recovered) {
        let mut got = Vec::new();
        let r = recover::<JournalError>(dir, |s, c| {
            got.push((s.get(), c));
            Ok(())
        })
        .unwrap();
        (got, r)
    }

    fn last_segment(dir: &Path) -> PathBuf {
        segments(dir).unwrap().pop().unwrap().1
    }

    #[test]
    fn everything_written_is_read_back_in_order_across_segments() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 5_000, 37, 4_096);
        assert!(
            segments(dir.path()).unwrap().len() > 10,
            "small segments force rotation"
        );
        let (got, r) = read(dir.path());
        assert_eq!(got, (1..=5_000).map(|i| (i, cmd(i))).collect::<Vec<_>>());
        assert_eq!(
            r,
            Recovered {
                last_seq: Some(Seq::from_raw(5_000)),
                truncation: None
            }
        );
    }

    #[test]
    fn appends_must_be_in_sequence_and_batches_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = JournalWriter::open(dir.path(), Seq::FIRST).unwrap();
        assert!(matches!(
            w.append(Seq::from_raw(2), &cmd(2)),
            Err(JournalError::OutOfSequence { .. })
        ));
        let mut i = 1;
        loop {
            match w.append(Seq::from_raw(i), &cmd(i)) {
                Ok(()) => i += 1,
                Err(JournalError::BatchFull) => break,
                Err(e) => panic!("{e}"),
            }
        }
        assert!(w.pending_bytes() <= MAX_BATCH_BYTES);
        w.commit().unwrap();
        w.append(Seq::from_raw(i), &cmd(i)).unwrap();
    }

    #[test]
    fn recovery_then_append_continues_the_same_log() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 100, 10, SEGMENT_BYTES);
        let (_, r) = read(dir.path());
        let next = r.last_seq.unwrap().next().unwrap();
        let mut w = JournalWriter::open(dir.path(), next).unwrap();
        for i in 101..=150 {
            w.append(Seq::from_raw(i), &cmd(i)).unwrap();
        }
        w.commit().unwrap();
        let (got, _) = read(dir.path());
        assert_eq!(got.len(), 150);
        assert_eq!(got.last(), Some(&(150, cmd(150))));
    }

    #[test]
    fn damage_before_the_last_segment_is_corruption_not_a_torn_tail() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 2_000, 50, 4_096);
        let first = segments(dir.path()).unwrap().first().unwrap().1.clone();
        let mut bytes = fs::read(&first).unwrap();
        let i = HEADER_LEN + 20;
        bytes[i] ^= 0xFF;
        fs::write(&first, bytes).unwrap();
        let err = recover::<JournalError>(dir.path(), |_, _| Ok(())).unwrap_err();
        assert!(matches!(err, JournalError::Corrupt { .. }), "{err}");
    }

    #[test]
    fn a_torn_header_from_a_crash_during_rotation_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 100, 10, SEGMENT_BYTES);
        let torn = segment_path(dir.path(), Seq::from_raw(101));
        fs::write(&torn, &MAGIC[..5]).unwrap();
        let (got, r) = read(dir.path());
        assert_eq!(got.len(), 100);
        assert_eq!(r.truncation.map(|t| t.offset), Some(0));
        assert!(!torn.exists());
    }

    #[test]
    fn more_than_a_batch_of_damage_at_the_tail_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 10, 10, SEGMENT_BYTES);
        let mut f = OpenOptions::new()
            .append(true)
            .open(last_segment(dir.path()))
            .unwrap();
        f.write_all(&vec![0xAB; MAX_BATCH_BYTES + 1]).unwrap();
        let err = recover::<JournalError>(dir.path(), |_, _| Ok(())).unwrap_err();
        assert!(matches!(err, JournalError::Corrupt { .. }), "{err}");
    }

    #[test]
    fn a_valid_record_out_of_sequence_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), 5, 5, SEGMENT_BYTES);
        // Hand-build a well-formed record for seq 9 (the log expects 6).
        let mut payload = Vec::new();
        encode(&cmd(9), &mut payload);
        let mut rec = Vec::new();
        rec.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        rec.extend_from_slice(&record_checksum(9, &payload).to_le_bytes());
        rec.extend_from_slice(&9u64.to_le_bytes());
        rec.extend_from_slice(&payload);
        OpenOptions::new()
            .append(true)
            .open(last_segment(dir.path()))
            .unwrap()
            .write_all(&rec)
            .unwrap();
        let err = recover::<JournalError>(dir.path(), |_, _| Ok(())).unwrap_err();
        assert!(
            matches!(
                err,
                JournalError::Corrupt {
                    reason: "sequence number is not the next one",
                    ..
                }
            ),
            "{err}"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        /// Cutting the last segment anywhere leaves exactly a prefix of what was written,
        /// reported as a truncation, and the log stays appendable.
        #[test]
        fn a_torn_tail_anywhere_leaves_a_clean_prefix(n in 1u64..400, batch in 1u64..50, cut in any::<prop::sample::Index>()) {
            let dir = tempfile::tempdir().unwrap();
            write(dir.path(), n, batch, SEGMENT_BYTES);
            let path = last_segment(dir.path());
            let len = fs::metadata(&path).unwrap().len();
            let keep = HEADER_LEN as u64 + u64::try_from(cut.index(usize::try_from(len).unwrap() - HEADER_LEN + 1)).unwrap();
            OpenOptions::new().write(true).open(&path).unwrap().set_len(keep).unwrap();
            let (got, r) = read(dir.path());
            prop_assert_eq!(&got, &(1..=got.len() as u64).map(|i| (i, cmd(i))).collect::<Vec<_>>());
            if keep == len {
                prop_assert_eq!(got.len() as u64, n);
                prop_assert_eq!(r.truncation, None);
            }
            let next = Seq::from_raw(got.len() as u64 + 1);
            let mut w = JournalWriter::open(dir.path(), next).unwrap();
            w.append(next, &cmd(next.get())).unwrap();
            w.commit().unwrap();
            prop_assert_eq!(read(dir.path()).0.len(), got.len() + 1);
        }

        /// Flipping any bit of the last record truncates it as a torn tail; it is never misread
        /// as a different command.
        #[test]
        fn a_damaged_last_record_is_dropped_never_misread(n in 1u64..100, which in any::<prop::sample::Index>(), bit in 0u8..8) {
            let dir = tempfile::tempdir().unwrap();
            write(dir.path(), n, 7, SEGMENT_BYTES);
            let path = last_segment(dir.path());
            let mut bytes = fs::read(&path).unwrap();
            let record_start = bytes.len() - RECORD_HEADER_LEN - {
                let mut p = Vec::new();
                encode(&cmd(n), &mut p);
                p.len()
            };
            let i = record_start + which.index(bytes.len() - record_start);
            bytes[i] ^= 1 << bit;
            fs::write(&path, bytes).unwrap();
            // CRC-32C detects every single-bit error, so the flip always reads as a torn record.
            let r = recover::<JournalError>(dir.path(), |_, _| Ok(())).unwrap();
            prop_assert!(r.truncation.is_some());
            let (got, _) = read(dir.path());
            prop_assert_eq!(&got, &(1..n).map(|i| (i, cmd(i))).collect::<Vec<_>>());
        }
    }
}
