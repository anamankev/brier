//! `brier-replay`: read wire-format commands as JSON lines on stdin, apply them to a fresh
//! engine in order, and write one JSON line per event to stdout, then a `final_book` record.
//! The format is fixed in docs/protocol.md and shared with the Python reference engine.
//!
//! Exit codes: 0 success; 2 malformed input (with the line number on stderr); 3 the engine
//! halted (a sequence gap or a violated invariant).

use std::io::{self, BufRead, BufWriter, Write};
use std::process::ExitCode;

use brier_engine::{Engine, Sequenced};
use brier_types::Seq;
use brier_wire::{event_json, final_book, parse_command};

enum Failure {
    Input(String),
    Engine(String),
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Replay every command; with `check_every > 0`, also run the full invariant check
/// (I1-I5) after every `check_every` commands and at the end.
fn run(input: impl BufRead, output: impl Write, check_every: u64) -> Result<(), Failure> {
    let mut out = BufWriter::new(output);
    let mut engine = Engine::new();
    let mut events = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let number = index.saturating_add(1);
        let (seq, command) =
            parse_command(&line).map_err(|e| Failure::Input(format!("line {number}: {e}")))?;
        let seq = seq.ok_or_else(|| Failure::Input(format!("line {number}: missing seq")))?;
        events.clear();
        let cmd = Sequenced {
            seq: Seq::from_raw(seq),
            command,
        };
        engine
            .apply(&cmd, &mut events)
            .map_err(|e| Failure::Engine(format!("line {number}: {e}")))?;
        if check_every > 0 && seq.is_multiple_of(check_every) {
            engine
                .check_invariants()
                .map_err(|e| Failure::Engine(format!("line {number}: {e}")))?;
        }
        for e in &events {
            serde_json::to_writer(&mut out, &event_json(seq, e))
                .map_err(|e| Failure::Io(io::Error::other(e)))?;
            out.write_all(b"\n")?;
        }
    }
    if check_every > 0 {
        engine
            .check_invariants()
            .map_err(|e| Failure::Engine(format!("at end of input: {e}")))?;
    }
    let book = final_book(&engine).map_err(|e| Failure::Engine(e.to_string()))?;
    serde_json::to_writer(&mut out, &book).map_err(|e| Failure::Io(io::Error::other(e)))?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// `--check-every N` (optional): run the full invariant check every N commands.
fn parse_args() -> Result<u64, Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(0),
        [flag, n] if flag == "--check-every" => n
            .parse()
            .map_err(|_| Failure::Input(format!("--check-every needs a number, got {n:?}"))),
        _ => Err(Failure::Input(format!(
            "usage: brier-replay [--check-every N], got {args:?}"
        ))),
    }
}

fn main() -> ExitCode {
    let stdin = io::stdin();
    let result = parse_args().and_then(|every| run(stdin.lock(), io::stdout().lock(), every));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Input(msg)) => {
            eprintln!("brier-replay: malformed input: {msg}");
            ExitCode::from(2)
        }
        Err(Failure::Engine(msg)) => {
            eprintln!("brier-replay: engine halted: {msg}");
            ExitCode::from(3)
        }
        Err(Failure::Io(e)) => {
            eprintln!("brier-replay: i/o error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn replay(input: &str) -> Result<Vec<Value>, Failure> {
        let mut out = Vec::new();
        run(input.as_bytes(), &mut out, 1)?;
        Ok(String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect())
    }

    #[test]
    fn a_short_session_round_trips_through_the_wire_format() {
        let input = r#"{"seq":1,"cmd":"open_account","account":1}
{"seq":2,"cmd":"open_account","account":2}
{"seq":3,"cmd":"create_market","market":1}
{"seq":4,"cmd":"set_state","market":1,"state":"open"}
{"seq":5,"cmd":"place","account":1,"market":1,"side":"sell","price":60,"qty":5,"tif":"gtc","post_only":false}
{"seq":6,"cmd":"place","account":2,"market":1,"side":"buy","price":61,"qty":2,"tif":"ioc","post_only":false}
{"seq":7,"cmd":"place","account":2,"market":1,"side":"buy","price":0,"qty":2,"tif":"gtc","post_only":false}
"#;
        let lines = replay(input).ok().unwrap();
        // Expected values computed by hand from docs/protocol.md §5: account 1 sells YES it
        // does not hold, so each unit opens NO at 100 − 60 = 40 (reserve 5 × 40 = 200); the
        // trade of 2 at 60 moves buyer −120, seller +120 − 200 = −80, escrow +200.
        assert_eq!(
            lines,
            vec![
                json!({"seq":1,"ev":"account_opened","account":1,"cash":1_000_000}),
                json!({"seq":1,"ev":"entry","kind":"grant","legs":[["issuance",-1_000_000],["cash:1",1_000_000]]}),
                json!({"seq":2,"ev":"account_opened","account":2,"cash":1_000_000}),
                json!({"seq":2,"ev":"entry","kind":"grant","legs":[["issuance",-1_000_000],["cash:2",1_000_000]]}),
                json!({"seq":3,"ev":"market_created","market":1}),
                json!({"seq":4,"ev":"market_state","market":1,"state":"open"}),
                json!({"seq":5,"ev":"accepted","order":5}),
                json!({"seq":5,"ev":"rested","order":5,"remaining":5}),
                json!({"seq":5,"ev":"reserved","account":1,"market":1,"amount":200}),
                json!({"seq":6,"ev":"accepted","order":6}),
                json!({"seq":6,"ev":"trade","market":1,"maker":5,"taker":6,"maker_account":1,
                       "taker_account":2,"taker_side":"buy","price":60,"qty":2}),
                json!({"seq":6,"ev":"entry","kind":"trade","legs":[["cash:2",-120],["cash:1",-80],["escrow:1",200]]}),
                json!({"seq":6,"ev":"reserved","account":1,"market":1,"amount":120}),
                json!({"seq":7,"ev":"rejected","reason":"invalid_price"}),
                json!({"ev":"final_book",
                       "markets":[{"market":1,"state":"open","bids":[],"asks":[[5,1,60,3]]}],
                       "accounts":[{"account":1,"cash":999_920,"reserved":120,"positions":[[1,-2]]},
                                   {"account":2,"cash":999_880,"reserved":0,"positions":[[1,2]]}],
                       "escrow":[[1,200]]}),
            ]
        );
    }

    #[test]
    fn malformed_lines_and_sequence_gaps_are_failures_not_events() {
        assert!(matches!(
            replay("{\"seq\":1,\"cmd\":\"nope\"}\n"),
            Err(Failure::Input(_))
        ));
        assert!(matches!(
            replay(
                "{\"seq\":1,\"cmd\":\"place\",\"account\":1,\"market\":1,\"side\":\"buy\",\"price\":300,\"qty\":1,\"tif\":\"gtc\",\"post_only\":false}\n"
            ),
            Err(Failure::Input(_))
        ));
        assert!(matches!(
            replay("{\"seq\":2,\"cmd\":\"create_market\",\"market\":1}\n"),
            Err(Failure::Engine(_))
        ));
    }
}
