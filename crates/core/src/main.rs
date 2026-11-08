//! `brier-core`: command-line front end to the core library.
//!
//! `brier-core serve --data DIR [--snapshot-every N] [--segment-bytes N]`
//!   Recover, print a `ready` line, then read wire commands as JSON lines on stdin (any `seq`
//!   field is ignored: the core is the sequencer). Submission is pipelined: commands that
//!   arrive together form one batch, synced to the device once (`F_FULLFSYNC` on macOS). Each
//!   command's `{"ack": seq}` and its events are printed, in order, only after its batch is
//!   durable. A malformed line gets `{"error": ...}` and no sequence number. Exit codes: 0 on
//!   end of input; 3 if the engine halts (the unsynced batch is dropped, never acknowledged);
//!   4 if the journal fails.
//!
//! `brier-core verify --data DIR`
//!   Replay the whole journal from empty, without snapshots and without modifying anything.
//!   Run the full invariant check, and print the state hash.
//!
//! `brier-core export --data DIR [--from SEQ]`
//!   Print every journalled command from `SEQ` (default 1) as a wire command line with its
//!   `seq`, without modifying anything: the input to surveillance (docs/surveillance.md).
//!   Replaying the output through `brier-replay` or the reference engine rebuilds the same
//!   state. A record still being written (a torn tail) is not printed.
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

use brier_core::{Committed, CoreConfig, CoreError, Recovery, start};
use brier_engine::{Command, Engine, Sequenced};
use brier_journal::{JournalError, JournalWriter, SEGMENT_BYTES, read_journal};
use brier_types::{AccountId, Seq};
use brier_wire::{command_json, event_json, parse_command};
use serde_json::json;
use tokio::sync::oneshot;

/// Bytes appended per sync in `sync-bench` (the durability note's registered 4 KiB).
const SYNC_BENCH_BYTES: usize = 4_096;

enum Failure {
    Usage(String),
    Core(CoreError),
    Io(io::Error),
}

impl From<CoreError> for Failure {
    fn from(e: CoreError) -> Self {
        Self::Core(e)
    }
}

impl From<JournalError> for Failure {
    fn from(e: JournalError) -> Self {
        Self::Core(CoreError::Journal(e))
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn engine_failure(e: impl std::fmt::Display) -> Failure {
    Failure::Core(CoreError::Engine(e.to_string()))
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        // Writing to a String cannot fail.
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn ready_json(r: &Recovery) -> serde_json::Value {
    json!({ "ready": {
        "next_seq": r.next_seq.get(),
        "state_hash": hex(&r.state_hash),
        "snapshot": r.snapshot.map(Seq::get),
        "skipped_snapshots": r.skipped_snapshots.iter().map(|(p, why)| json!([p.display().to_string(), why])).collect::<Vec<_>>(),
        "replayed": r.replayed,
        "truncation": r.truncation.as_ref().map(|t| json!({
            "segment": t.segment.display().to_string(), "offset": t.offset, "dropped": t.dropped,
        })),
    }})
}

/// What the printer thread receives, in input order.
enum Pending {
    Ack(oneshot::Receiver<Result<Committed, CoreError>>),
    Malformed(String),
}

fn serve(config: CoreConfig) -> Result<(), Failure> {
    let (handle, recovery, core) = start(config)?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(out, "{}", ready_json(&recovery))?;
    out.flush()?;

    // Reader: submit each command without waiting, so commands that arrive while a batch is
    // syncing join the next batch (group commit, ADR-0004).
    let (pending_tx, pending_rx) = mpsc::channel::<Pending>();
    let reader = thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let item = match parse_command(&line) {
                Ok((_, command)) => match handle.submit_nowait(command) {
                    Ok(receiver) => Pending::Ack(receiver),
                    Err(_) => break, // the core stopped; its thread reports why
                },
                Err(e) => Pending::Malformed(e.to_string()),
            };
            if pending_tx.send(item).is_err() {
                break;
            }
        }
        // Dropping the handle here lets the core thread finish once its queue is empty.
    });

    // Printer (this thread): print in input order, each ack only once it is durable.
    for item in pending_rx {
        match item {
            Pending::Malformed(e) => writeln!(out, "{}", json!({ "error": e }))?,
            Pending::Ack(receiver) => {
                let Ok(Ok(c)) = receiver.blocking_recv() else {
                    break;
                };
                writeln!(out, "{}", json!({ "ack": c.seq.get() }))?;
                for e in &c.events {
                    writeln!(out, "{}", event_json(c.seq.get(), e))?;
                }
            }
        }
        out.flush()?;
    }
    out.flush()?;
    let _ = reader.join();
    match core.join() {
        Ok(result) => Ok(result?),
        Err(_) => Err(Failure::Core(CoreError::Engine(
            "core thread panicked".to_owned(),
        ))),
    }
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

fn export(data: &Path, from: u64) -> Result<(), Failure> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    read_journal::<Failure>(data, |seq, command| {
        if seq.get() >= from {
            writeln!(out, "{}", command_json(seq.get(), &command))?;
        }
        Ok(())
    })?;
    out.flush()?;
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
            "usage: brier-core serve --data DIR [--snapshot-every N] [--segment-bytes N] | verify --data DIR | export --data DIR [--from SEQ] | sync-bench --data DIR --count N"
                .to_owned(),
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
        Some("serve") => {
            let mut config = CoreConfig::new(&data);
            config.snapshot_every = number("--snapshot-every", config.snapshot_every)?.max(1);
            config.segment_bytes = number("--segment-bytes", SEGMENT_BYTES)?.max(1);
            serve(config)
        }
        Some("verify") => verify(&data),
        Some("export") => export(&data, number("--from", 1)?),
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
        Err(Failure::Core(CoreError::Engine(msg))) => {
            eprintln!("brier-core: engine halted: {msg}");
            ExitCode::from(3)
        }
        Err(Failure::Core(e)) => {
            eprintln!("brier-core: {e}");
            ExitCode::from(4)
        }
        Err(Failure::Io(e)) => {
            eprintln!("brier-core: i/o error: {e}");
            ExitCode::from(4)
        }
    }
}
