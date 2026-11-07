//! Every request body and WebSocket message a client can send must decode or be refused with
//! a client error; it must never panic, and a decoded order must belong to the caller.
#![no_main]
use brier_engine::Command;
use brier_gateway::routes::decode_order;
use brier_gateway::ws::{order_command, parse_message};
use brier_gateway::feed::Channel;
use brier_types::AccountId;
use libfuzzer_sys::fuzz_target;

const CALLER: AccountId = AccountId::new(42);

fn owned_by_caller(command: &Command) -> bool {
    match *command {
        Command::Place(n) => n.account == CALLER,
        Command::Cancel { account, .. } | Command::Amend { account, .. } => account == CALLER,
        _ => false,
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(command) = decode_order(data, CALLER) {
        assert!(owned_by_caller(&command), "a REST order decoded for another account");
    }
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = Channel::parse(text, None);
    let _ = Channel::parse(text, Some(CALLER));
    match parse_message(text) {
        Ok((op, _, body)) => {
            if let Ok(command) = order_command(&op, body, CALLER) {
                assert!(owned_by_caller(&command), "a WebSocket order decoded for another account");
            }
        }
        Err((_, e)) => assert_eq!(e.status().as_u16(), 400),
    }
});
