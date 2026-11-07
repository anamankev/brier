//! `brier-core`: the exchange's core process (SPEC §5, §12; ADR-0004).
//!
//! `brier-core serve --data DIR [--snapshot-every N] [--segment-bytes N]`
//!   Recover (newest valid snapshot, then the journal after it), print a `ready` line, then
//!   read wire commands as JSON lines on stdin (any `seq` field is ignored: the core is the
//!   sequencer). Commands that arrive together form one batch:
//!   1. each gets the next sequence number, is appended to the journal, and is applied;
//!   2. the batch is synced to the device (one `F_FULLFSYNC` on macOS);
//!   3. only then is `{"ack": seq}` printed for each command, followed by its events.
//!
//!   A malformed line gets `{"error": ...}` and no sequence number. Exit codes: 0 on end of
//!   input; 3 if the engine halts (the unsynced batch is dropped, never acknowledged); 4 if
//!   the journal fails.
//!
//! `brier-core verify --data DIR`
//!   Replay the whole journal from empty, without snapshots and without modifying anything.
//!   Run the full invariant check, and print the state hash.
//!
//! `brier-core sync-bench --data DIR --count N`
//!   Time N append-and-sync cycles of 4 KiB each (the durability note's sync cost).

use std::fmt::Write as _;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use brier_engine::{Command, Engine, Event, Sequenced};
use brier_journal::{
    JournalError, JournalWriter, SEGMENT_BYTES, latest_snapshot, read_journal, recover_after,
    write_snapshot,
};
use brier_types::{AccountId, Seq};
use brier_wire::{event_json, parse_command};
use serde_json::json;

/// Most commands in one group-commit batch. 8,192 records of at most 80 bytes stay under the
/// journal's `MAX_BATCH_BYTES` (1 MiB), so a full batch can never be refused.
const MAX_BATCH_COMMANDS: usize = 8_192;
/// Default commands between snapshots.
const DEFAULT_SNAPSHOT_EVERY: u64 = 100_000;
/// Bytes appended per sync in `sync-bench` (the durability note's registered 4 KiB).
const SYNC_BENCH_BYTES: usize = 4_096;

enum Failure {
    Usage(String),
    Journal(JournalError),
    Engine(String),
    Io(io::Error),
}

impl From<JournalError> for Failure {
    fn from(e: JournalError) -> Self {
        Self::Journal(e)
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        // Writing to a String cannot fail.
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn engine_failure(e: impl std::fmt::Display) -> Failure {
    Failure::Engine(e.to_string())
}

/// Rebuild the engine from the newest valid snapshot plus the journal after it.
fn recover_engine(data: &Path) -> Result<(Engine, Seq, serde_json::Value), Failure> {
    std::fs::create_dir_all(data)?;
    let latest = latest_snapshot(data)?;
    let (mut engine, snapshot_seq) = match &latest.snapshot {
        Some(s) => {
            let engine = Engine::restore(&s.body).map_err(engine_failure)?;
            if engine.state_hash().map_err(engine_failure)? != s.state_hash {
                return Err(Failure::Engine(format!(
                    "snapshot {} restores to a different state hash than it recorded",
                    s.seq.get()
                )));
            }
            (engine, s.seq.get())
        }
        None => (Engine::new(), 0),
    };
    let mut events = Vec::new();
    let mut replayed: u64 = 0;
    let recovered = recover_after::<Failure>(data, Seq::from_raw(snapshot_seq), |seq, command| {
        if seq.get() <= snapshot_seq {
            return Ok(());
        }
        events.clear();
        engine
            .apply(&Sequenced { seq, command }, &mut events)
            .map_err(engine_failure)?;
        replayed = replayed.saturating_add(1);
        Ok(())
    })?;
    let last = recovered.last_seq.map_or(0, Seq::get);
    if last < snapshot_seq {
        return Err(Failure::Engine(format!(
            "snapshot at {snapshot_seq} is ahead of the journal, which ends at {last}"
        )));
    }
    let report = json!({
        "next_seq": last.saturating_add(1),
        "state_hash": hex(&engine.state_hash().map_err(engine_failure)?),
        "snapshot": (snapshot_seq > 0).then_some(snapshot_seq),
        "skipped_snapshots": latest.skipped.iter().map(|(p, why)| json!([p.display().to_string(), why])).collect::<Vec<_>>(),
        "replayed": replayed,
        "truncation": recovered.truncation.map(|t| json!({
            "segment": t.segment.display().to_string(), "offset": t.offset, "dropped": t.dropped,
        })),
    });
    Ok((engine, Seq::from_raw(last.saturating_add(1)), report))
}

fn serve(data: &Path, snapshot_every: u64, segment_bytes: u64) -> Result<(), Failure> {
    let (mut engine, next, report) = recover_engine(data)?;
    let mut journal = JournalWriter::open_with_segment_limit(data, next, segment_bytes)?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(out, "{}", json!({ "ready": report }))?;
    out.flush()?;

    // Lines arrive on their own thread, so commands that come in while a batch is syncing
    // queue up and form the next batch (group commit, ADR-0004).
    let (tx, rx) = mpsc::sync_channel::<String>(MAX_BATCH_COMMANDS.saturating_mul(4));
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut since_snapshot: u64 = 0;
    let mut events: Vec<Event> = Vec::new();
    let mut pending_output: Vec<u8> = Vec::new();
    while let Ok(first) = rx.recv() {
        let mut lines = vec![first];
        while lines.len() < MAX_BATCH_COMMANDS {
            match rx.try_recv() {
                Ok(line) => lines.push(line),
                Err(_) => break,
            }
        }
        pending_output.clear();
        let mut applied: u64 = 0;
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let command = match parse_command(&line) {
                Ok((_, command)) => command,
                Err(e) => {
                    writeln!(pending_output, "{}", json!({ "error": e.to_string() }))?;
                    continue;
                }
            };
            let seq = journal.next_seq();
            journal.append(seq, &command)?;
            events.clear();
            // A halt here drops the whole unsynced batch: nothing in it was acknowledged.
            engine
                .apply(&Sequenced { seq, command }, &mut events)
                .map_err(engine_failure)?;
            writeln!(pending_output, "{}", json!({ "ack": seq.get() }))?;
            for e in &events {
                writeln!(pending_output, "{}", event_json(seq.get(), e))?;
            }
            applied = applied.saturating_add(1);
        }
        journal.commit()?;
        out.write_all(&pending_output)?;
        out.flush()?;
        since_snapshot = since_snapshot.saturating_add(applied);
        if since_snapshot >= snapshot_every {
            let last = Seq::from_raw(journal.next_seq().get().saturating_sub(1));
            let body = engine.snapshot().map_err(engine_failure)?;
            write_snapshot(
                data,
                last,
                &engine.state_hash().map_err(engine_failure)?,
                &body,
            )?;
            since_snapshot = 0;
        }
    }
    journal.commit()?;
    Ok(())
}

fn verify(data: &Path) -> Result<(), Failure> {
    let start = Instant::now();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    let mut commands: u64 = 0;
    let recovered = read_journal::<Failure>(data, |seq, command| {
        events.clear();
        engine
            .apply(&Sequenced { seq, command }, &mut events)
            .map_err(engine_failure)?;
        commands = commands.saturating_add(1);
        Ok(())
    })?;
    let replay_seconds = start.elapsed().as_secs_f64();
    engine.check_invariants().map_err(engine_failure)?;
    println!(
        "{}",
        json!({ "verified": {
            "last_seq": recovered.last_seq.map(Seq::get),
            "commands": commands,
            "state_hash": hex(&engine.state_hash().map_err(engine_failure)?),
            "replay_seconds": replay_seconds,
            "torn_tail": recovered.truncation.is_some(),
        }})
    );
    Ok(())
}

fn sync_bench(data: &Path, count: u64) -> Result<(), Failure> {
    std::fs::create_dir_all(data)?;
    let mut journal = JournalWriter::open(data, Seq::FIRST)?;
    let mut nanos: Vec<u128> = Vec::new();
    let mut seq = Seq::FIRST;
    for _ in 0..count {
        // About 4 KiB of records per sync, as registered in the durability note.
        while journal.pending_bytes() < SYNC_BENCH_BYTES {
            let account = AccountId::new(seq.get());
            journal.append(seq, &Command::OpenAccount { account })?;
            seq = seq.next().map_err(engine_failure)?;
        }
        let start = Instant::now();
        journal.commit()?;
        nanos.push(start.elapsed().as_nanos());
    }
    nanos.sort_unstable();
    let at = |q_per_mille: usize| -> u128 {
        let last = nanos.len().saturating_sub(1);
        nanos
            .get(last.saturating_mul(q_per_mille) / 1000)
            .copied()
            .unwrap_or(0)
    };
    println!(
        "{}",
        json!({ "sync_bench": {
            "count": count,
            "p50_us": at(500) / 1000, "p90_us": at(900) / 1000, "p99_us": at(990) / 1000,
            "max_us": nanos.last().copied().unwrap_or(0) / 1000,
        }})
    );
    Ok(())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i.saturating_add(1)))
        .cloned()
}

fn run() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        Failure::Usage(
        "usage: brier-core serve --data DIR [--snapshot-every N] [--segment-bytes N] | verify --data DIR | sync-bench --data DIR --count N".to_owned(),
    )
    };
    let data = PathBuf::from(flag(&args, "--data").ok_or_else(usage)?);
    let number = |name: &str, default: u64| -> Result<u64, Failure> {
        flag(&args, name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| Failure::Usage(format!("{name} needs a number")))
        })
    };
    match args.first().map(String::as_str) {
        Some("serve") => serve(
            &data,
            number("--snapshot-every", DEFAULT_SNAPSHOT_EVERY)?.max(1),
            number("--segment-bytes", SEGMENT_BYTES)?.max(1),
        ),
        Some("verify") => verify(&data),
        Some("sync-bench") => sync_bench(&data, number("--count", 1_000)?),
        _ => Err(usage()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(msg)) => {
            eprintln!("brier-core: {msg}");
            ExitCode::from(2)
        }
        Err(Failure::Engine(msg)) => {
            eprintln!("brier-core: engine halted: {msg}");
            ExitCode::from(3)
        }
        Err(Failure::Journal(e)) => {
            eprintln!("brier-core: {e}");
            ExitCode::from(4)
        }
        Err(Failure::Io(e)) => {
            eprintln!("brier-core: i/o error: {e}");
            ExitCode::from(4)
        }
    }
}
