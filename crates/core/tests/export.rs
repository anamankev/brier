//! `brier-core export` writes the journal back out as wire commands that rebuild the same
//! state (G8: surveillance reads live markets through this).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::format_collect
)] // a failure must stop the test

use std::process::Command as Process;

use brier_core::{CoreConfig, start};
use brier_engine::{Command, Engine, MarketState, NewOrder, Sequenced, TimeInForce};
use brier_types::{AccountId, Digest, MarketId, OrderId, Outcome, Seq, Side};
use brier_wire::parse_command;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_brier-core");

fn place(account: u64, side: Side, price: u8, qty: u32, tif: TimeInForce) -> Command {
    Command::Place(NewOrder {
        account: AccountId::new(account),
        market: MarketId::new(1),
        side,
        price,
        qty,
        tif,
        post_only: false,
        client_order_id: Some(account * 1000 + u64::from(price)),
    })
}

fn history() -> Vec<Command> {
    let m = MarketId::new(1);
    let mut out = vec![
        Command::OpenAccount {
            account: AccountId::new(1),
        },
        Command::OpenAccount {
            account: AccountId::new(2),
        },
        Command::CreateMarket {
            market: m,
            rules_hash: Some(Digest::new([7; 32])),
        },
        Command::SetState {
            market: m,
            state: MarketState::Open,
        },
    ];
    for i in 0..30u8 {
        out.push(place(1, Side::Sell, 40 + i % 20, 3, TimeInForce::Gtc));
        out.push(place(2, Side::Buy, 45 + i % 10, 2, TimeInForce::Ioc));
    }
    out.extend([
        Command::Cancel {
            account: AccountId::new(1),
            order: OrderId::new(3),
        },
        Command::Amend {
            account: AccountId::new(1),
            order: OrderId::new(5),
            qty: 1,
        },
        Command::CancelAll {
            account: Some(AccountId::new(1)),
            market: None,
        },
        Command::SetState {
            market: m,
            state: MarketState::Closed,
        },
        Command::Settle {
            market: m,
            outcome: Outcome::Yes,
            evidence_hash: Some(Digest::new([9; 32])),
        },
    ]);
    out
}

fn stdout(args: &[&str]) -> String {
    let out = Process::new(BIN).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn an_exported_journal_replays_to_the_same_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = CoreConfig::new(dir.path());
    config.snapshot_every = 16; // snapshots do not change what is exported
    let (handle, _, core) = start(config).unwrap();
    let commands = history();
    for c in &commands {
        handle.submit(*c).await.unwrap();
    }
    drop(handle);
    core.join().unwrap().unwrap();

    let data = dir.path().to_str().unwrap();
    let exported = stdout(&["export", "--data", data]);
    let lines: Vec<&str> = exported.lines().collect();
    assert_eq!(lines.len(), commands.len());

    let mut engine = Engine::new();
    let mut events = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let (seq, command) = parse_command(line).unwrap();
        assert_eq!(seq, Some(i as u64 + 1));
        assert_eq!(
            command, commands[i],
            "the exported command is the submitted one"
        );
        engine
            .apply(
                &Sequenced {
                    seq: Seq::from_raw(seq.unwrap()),
                    command,
                },
                &mut events,
            )
            .unwrap();
    }
    let verified: Value = serde_json::from_str(stdout(&["verify", "--data", data]).trim()).unwrap();
    let hash: String = engine
        .state_hash()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        verified["verified"]["state_hash"].as_str(),
        Some(hash.as_str())
    );

    // --from skips earlier commands.
    let tail = stdout(&["export", "--data", data, "--from", "60"]);
    let first: Value = serde_json::from_str(tail.lines().next().unwrap()).unwrap();
    assert_eq!(
        (first["seq"].as_u64(), tail.lines().count()),
        (Some(60), commands.len() - 59)
    );
}
