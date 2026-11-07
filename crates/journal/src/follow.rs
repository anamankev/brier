//! Following the journal as it grows, up to the durable sequence number (G4 plan, read models).
//!
//! A [`Follower`] hands each record to its caller exactly once, in order, and never one beyond
//! the `durable` bound it is given. Bytes past that bound may belong to a batch that is not yet
//! synced, which a crash could still undo; a read model built from them could show orders that
//! never happened. The core publishes the bound after every sync (`CoreHandle::durable`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use brier_engine::Command;
use brier_types::Seq;

use crate::log::{JournalError, parse_record, segment_header, segments};

/// Reads journal records in order, resuming where it left off.
pub struct Follower {
    dir: PathBuf,
    next: Seq,
    /// The segment being read: its path, format version, and the offset of the next record.
    current: Option<(PathBuf, u16, u64)>,
}

impl Follower {
    /// A follower whose first record will be `next` (1 to read the whole journal).
    #[must_use]
    pub fn new(dir: &Path, next: Seq) -> Self {
        Self {
            dir: dir.to_path_buf(),
            next,
            current: None,
        }
    }

    /// The sequence number of the next record this follower will hand out.
    #[must_use]
    pub const fn next_seq(&self) -> Seq {
        self.next
    }

    /// Hand every record from `next_seq` up to and including `durable` to `apply`, in order.
    /// Returns how many were handed out.
    ///
    /// # Errors
    /// [`JournalError::Corrupt`] if a durable record is missing or damaged, [`JournalError::Io`]
    /// for file-system failures, or whatever `apply` returns, converted.
    pub fn poll<E>(
        &mut self,
        durable: u64,
        mut apply: impl FnMut(Seq, Command) -> Result<(), E>,
    ) -> Result<u64, E>
    where
        E: From<JournalError>,
    {
        let mut handed: u64 = 0;
        while self.next.get() <= durable {
            let (path, version, offset) = match self.current.take() {
                Some(c) => c,
                None => self.locate()?,
            };
            let bytes = read_from(&path, offset)?;
            let mut used: usize = 0;
            while self.next.get() <= durable {
                let Some(rest) = bytes.get(used..).filter(|r| !r.is_empty()) else {
                    break;
                };
                let Some((len, seq, command)) = parse_record(rest, version) else {
                    break; // the rest of this file is not yet complete, or the record is in the next segment
                };
                if seq != self.next.get() {
                    return Err(
                        corrupt(&path, offset, "record out of sequence while following").into(),
                    );
                }
                apply(self.next, command)?;
                handed = handed.saturating_add(1);
                self.next = self
                    .next
                    .next()
                    .map_err(|_| corrupt(&path, offset, "sequence overflow"))?;
                used = used.saturating_add(len);
            }
            let offset = offset.saturating_add(used as u64);
            if self.next.get() > durable {
                self.current = Some((path, version, offset));
                break;
            }
            // Durable records remain but this segment has no more complete records: they must
            // be in the next segment, which starts exactly at `next`.
            let next_segment = segments(&self.dir)?
                .into_iter()
                .find(|(first, _)| *first == self.next.get());
            match next_segment {
                Some((_, next_path)) => {
                    let (version, header_len) = segment_header(&next_path)?;
                    self.current = Some((next_path, version, header_len));
                }
                None => {
                    return Err(corrupt(
                        &path,
                        offset,
                        "a durable record is missing from the journal",
                    )
                    .into());
                }
            }
        }
        Ok(handed)
    }

    /// Find the segment holding `next` and skip to it.
    fn locate(&self) -> Result<(PathBuf, u16, u64), JournalError> {
        let files = segments(&self.dir)?;
        let (_, path) = files
            .into_iter()
            .take_while(|(first, _)| *first <= self.next.get())
            .last()
            .ok_or_else(|| {
                corrupt(
                    &self.dir,
                    0,
                    "no segment holds the requested sequence number",
                )
            })?;
        let (version, header_len) = segment_header(&path)?;
        let bytes = read_from(&path, header_len)?;
        let mut used: usize = 0;
        while let Some(rest) = bytes.get(used..).filter(|r| !r.is_empty()) {
            let Some((len, seq, _)) = parse_record(rest, version) else {
                break;
            };
            if seq >= self.next.get() {
                break;
            }
            used = used.saturating_add(len);
        }
        Ok((path, version, header_len.saturating_add(used as u64)))
    }
}

fn read_from(path: &Path, offset: u64) -> Result<Vec<u8>, JournalError> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn corrupt(path: &Path, offset: u64, reason: &'static str) -> JournalError {
    JournalError::Corrupt {
        segment: path.to_path_buf(),
        offset,
        reason,
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)] // test bookkeeping
mod tests {
    use super::*;
    use crate::log::JournalWriter;
    use brier_types::AccountId;

    fn cmd(i: u64) -> Command {
        Command::OpenAccount {
            account: AccountId::new(i),
        }
    }

    fn collect(f: &mut Follower, durable: u64) -> Vec<u64> {
        let mut got = Vec::new();
        f.poll::<JournalError>(durable, |s, c| {
            assert_eq!(c, cmd(s.get()));
            got.push(s.get());
            Ok(())
        })
        .unwrap();
        got
    }

    #[test]
    fn hands_out_each_record_once_in_order_and_never_past_durable() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = JournalWriter::open_with_segment_limit(dir.path(), Seq::FIRST, 512).unwrap();
        let mut f = Follower::new(dir.path(), Seq::FIRST);
        let mut all = Vec::new();
        let mut written = 0;
        for batch in [7, 1, 40, 3, 100] {
            for _ in 0..batch {
                written += 1;
                w.append(Seq::from_raw(written), &cmd(written)).unwrap();
            }
            w.commit().unwrap();
            // Follow in two steps: first part-way, then to the end of what is durable.
            all.extend(collect(&mut f, written - 1));
            all.extend(collect(&mut f, written));
            assert_eq!(f.next_seq().get(), written + 1);
        }
        assert_eq!(all, (1..=written).collect::<Vec<_>>());
        assert!(
            segments(dir.path()).unwrap().len() >= 3,
            "rotation happens after a commit that crosses 512 bytes: batches 3 and 5 here"
        );
        assert_eq!(collect(&mut f, written), Vec::<u64>::new(), "nothing new");
    }

    #[test]
    fn can_start_in_the_middle_of_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = JournalWriter::open_with_segment_limit(dir.path(), Seq::FIRST, 512).unwrap();
        for i in 1..=200 {
            w.append(Seq::from_raw(i), &cmd(i)).unwrap();
        }
        w.commit().unwrap();
        let mut f = Follower::new(dir.path(), Seq::from_raw(137));
        assert_eq!(collect(&mut f, 200), (137..=200).collect::<Vec<_>>());
    }

    #[test]
    fn a_durable_bound_beyond_the_journal_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = JournalWriter::open(dir.path(), Seq::FIRST).unwrap();
        w.append(Seq::FIRST, &cmd(1)).unwrap();
        w.commit().unwrap();
        let mut f = Follower::new(dir.path(), Seq::FIRST);
        let err = f.poll::<JournalError>(2, |_, _| Ok(())).unwrap_err();
        assert!(
            matches!(
                err,
                JournalError::Corrupt {
                    reason: "a durable record is missing from the journal",
                    ..
                }
            ),
            "{err}"
        );
    }
}
