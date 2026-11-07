//! Canonical binary encoding of engine commands for the journal (ADR-0006).
//!
//! One tag byte per command kind, then that command's fields at fixed widths, little-endian.
//! Optional fields carry a presence byte (0 or 1). Decoding is strict: an unknown tag, an
//! unknown enum code, a presence byte other than 0 or 1, a short payload, or trailing bytes is
//! an error. One command has exactly one encoding.

use std::fmt;

use brier_engine::{Command, MarketState, NewOrder, TimeInForce};
use brier_types::{AccountId, Digest, MarketId, OrderId, Outcome, Side};

/// The journal format version this code writes (ADR-0006). Version 2 added client order IDs
/// to `place`; version 3 added the rules hash to `create_market` and the evidence hash to
/// `settle`. Versions 1 and 2 are still read.
pub const FORMAT_VERSION: u16 = 3;

const TAG_CREATE_MARKET: u8 = 1;
const TAG_SET_STATE: u8 = 2;
const TAG_PLACE: u8 = 3;
const TAG_CANCEL: u8 = 4;
const TAG_AMEND: u8 = 5;
const TAG_OPEN_ACCOUNT: u8 = 6;
const TAG_SETTLE: u8 = 7;
const TAG_CANCEL_ALL: u8 = 8;
const TAG_HALT_ALL: u8 = 9;

/// Why a payload could not be decoded into a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The payload ended before the command's fields did.
    Truncated,
    /// Bytes remained after a complete command.
    TrailingBytes,
    /// An unknown command tag.
    UnknownTag(u8),
    /// A field held a value outside its code list. Names the field.
    BadCode(&'static str),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("payload is truncated"),
            Self::TrailingBytes => f.write_str("payload has trailing bytes"),
            Self::UnknownTag(t) => write!(f, "unknown command tag {t}"),
            Self::BadCode(field) => write!(f, "invalid code in field {field}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// Append the canonical encoding of `command` to `out`, in the current journal format.
pub fn encode(command: &Command, out: &mut Vec<u8>) {
    match *command {
        Command::CreateMarket { market, rules_hash } => {
            out.push(TAG_CREATE_MARKET);
            out.extend_from_slice(&market.get().to_le_bytes());
            digest(rules_hash, out);
        }
        Command::SetState { market, state } => {
            out.push(TAG_SET_STATE);
            out.extend_from_slice(&market.get().to_le_bytes());
            out.push(state.code());
        }
        Command::Place(o) => {
            out.push(TAG_PLACE);
            out.extend_from_slice(&o.account.get().to_le_bytes());
            out.extend_from_slice(&o.market.get().to_le_bytes());
            out.push(match o.side {
                Side::Buy => 0,
                Side::Sell => 1,
            });
            out.push(o.price);
            out.extend_from_slice(&o.qty.to_le_bytes());
            out.push(match o.tif {
                TimeInForce::Gtc => 0,
                TimeInForce::Ioc => 1,
                TimeInForce::Fok => 2,
            });
            out.push(u8::from(o.post_only));
            match o.client_order_id {
                Some(id) => {
                    out.push(1);
                    out.extend_from_slice(&id.to_le_bytes());
                }
                None => out.push(0),
            }
        }
        Command::Cancel { account, order } => {
            out.push(TAG_CANCEL);
            out.extend_from_slice(&account.get().to_le_bytes());
            out.extend_from_slice(&order.get().to_le_bytes());
        }
        Command::Amend {
            account,
            order,
            qty,
        } => {
            out.push(TAG_AMEND);
            out.extend_from_slice(&account.get().to_le_bytes());
            out.extend_from_slice(&order.get().to_le_bytes());
            out.extend_from_slice(&qty.to_le_bytes());
        }
        Command::OpenAccount { account } => {
            out.push(TAG_OPEN_ACCOUNT);
            out.extend_from_slice(&account.get().to_le_bytes());
        }
        Command::Settle {
            market,
            outcome,
            evidence_hash,
        } => {
            out.push(TAG_SETTLE);
            out.extend_from_slice(&market.get().to_le_bytes());
            out.push(match outcome {
                Outcome::Yes => 0,
                Outcome::No => 1,
                Outcome::Void => 2,
            });
            digest(evidence_hash, out);
        }
        Command::CancelAll { account, market } => {
            out.push(TAG_CANCEL_ALL);
            match account {
                Some(a) => {
                    out.push(1);
                    out.extend_from_slice(&a.get().to_le_bytes());
                }
                None => out.push(0),
            }
            match market {
                Some(m) => {
                    out.push(1);
                    out.extend_from_slice(&m.get().to_le_bytes());
                }
                None => out.push(0),
            }
        }
        Command::HaltAll => out.push(TAG_HALT_ALL),
    }
}

/// An optional digest: a presence byte, then the 32 bytes if present.
fn digest(d: Option<Digest>, out: &mut Vec<u8>) {
    match d {
        Some(d) => {
            out.push(1);
            out.extend_from_slice(d.bytes());
        }
        None => out.push(0),
    }
}

/// Reads fixed-width fields from a payload, failing instead of panicking when it runs out.
struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let (head, tail) = self.rest.split_at_checked(N).ok_or(CodecError::Truncated)?;
        self.rest = tail;
        head.try_into().map_err(|_| CodecError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(u8::from_le_bytes(self.take::<1>()?))
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(self.take::<8>()?))
    }

    fn flag(&mut self, field: &'static str) -> Result<bool, CodecError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::BadCode(field)),
        }
    }

    /// An optional digest, present only from format version 3.
    fn digest(&mut self, version: u16, field: &'static str) -> Result<Option<Digest>, CodecError> {
        if version >= 3 && self.flag(field)? {
            Ok(Some(Digest::new(self.take::<32>()?)))
        } else {
            Ok(None)
        }
    }

    fn finish(self) -> Result<(), CodecError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(CodecError::TrailingBytes)
        }
    }
}

fn state_from(code: u8) -> Result<MarketState, CodecError> {
    match code {
        0 => Ok(MarketState::PreOpen),
        1 => Ok(MarketState::Open),
        2 => Ok(MarketState::Halted),
        3 => Ok(MarketState::Closed),
        4 => Ok(MarketState::Resolved),
        5 => Ok(MarketState::Voided),
        _ => Err(CodecError::BadCode("state")),
    }
}

/// Decode exactly one command from `payload`, written in journal format `version`.
/// Version 1 has no client order ID; version 2 adds it to `place`; version 3 adds the rules
/// and evidence hashes (ADR-0006).
///
/// # Errors
/// A [`CodecError`] if the payload is not the canonical encoding of a command.
pub fn decode(payload: &[u8], version: u16) -> Result<Command, CodecError> {
    let mut r = Reader { rest: payload };
    let command = match r.u8()? {
        TAG_CREATE_MARKET => {
            let market = MarketId::new(r.u32()?);
            Command::CreateMarket {
                market,
                rules_hash: r.digest(version, "rules hash presence")?,
            }
        }
        TAG_SET_STATE => {
            let market = MarketId::new(r.u32()?);
            Command::SetState {
                market,
                state: state_from(r.u8()?)?,
            }
        }
        TAG_PLACE => {
            let account = AccountId::new(r.u64()?);
            let market = MarketId::new(r.u32()?);
            let side = match r.u8()? {
                0 => Side::Buy,
                1 => Side::Sell,
                _ => return Err(CodecError::BadCode("side")),
            };
            let price = r.u8()?;
            let qty = r.u32()?;
            let tif = match r.u8()? {
                0 => TimeInForce::Gtc,
                1 => TimeInForce::Ioc,
                2 => TimeInForce::Fok,
                _ => return Err(CodecError::BadCode("tif")),
            };
            let post_only = r.flag("post_only")?;
            let client_order_id = if version >= 2 && r.flag("client order ID presence")? {
                Some(r.u64()?)
            } else {
                None
            };
            Command::Place(NewOrder {
                account,
                market,
                side,
                price,
                qty,
                tif,
                post_only,
                client_order_id,
            })
        }
        TAG_CANCEL => {
            let account = AccountId::new(r.u64()?);
            Command::Cancel {
                account,
                order: OrderId::new(r.u64()?),
            }
        }
        TAG_AMEND => {
            let account = AccountId::new(r.u64()?);
            let order = OrderId::new(r.u64()?);
            Command::Amend {
                account,
                order,
                qty: r.u32()?,
            }
        }
        TAG_OPEN_ACCOUNT => Command::OpenAccount {
            account: AccountId::new(r.u64()?),
        },
        TAG_SETTLE => {
            let market = MarketId::new(r.u32()?);
            let outcome = match r.u8()? {
                0 => Outcome::Yes,
                1 => Outcome::No,
                2 => Outcome::Void,
                _ => return Err(CodecError::BadCode("outcome")),
            };
            Command::Settle {
                market,
                outcome,
                evidence_hash: r.digest(version, "evidence hash presence")?,
            }
        }
        TAG_CANCEL_ALL => {
            let account = if r.flag("account presence")? {
                Some(AccountId::new(r.u64()?))
            } else {
                None
            };
            let market = if r.flag("market presence")? {
                Some(MarketId::new(r.u32()?))
            } else {
                None
            };
            Command::CancelAll { account, market }
        }
        TAG_HALT_ALL => Command::HaltAll,
        tag => return Err(CodecError::UnknownTag(tag)),
    };
    r.finish()?;
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn command() -> impl Strategy<Value = Command> {
        let account = any::<u64>().prop_map(AccountId::new);
        let market = any::<u32>().prop_map(MarketId::new);
        let state = (0u8..=5).prop_map(|c| state_from(c).unwrap());
        let side = prop_oneof![Just(Side::Buy), Just(Side::Sell)];
        let tif = prop_oneof![
            Just(TimeInForce::Gtc),
            Just(TimeInForce::Ioc),
            Just(TimeInForce::Fok)
        ];
        let outcome = prop_oneof![Just(Outcome::Yes), Just(Outcome::No), Just(Outcome::Void)];
        let digest = || prop::option::of(any::<[u8; 32]>().prop_map(Digest::new));
        prop_oneof![
            (market.clone(), digest())
                .prop_map(|(market, rules_hash)| Command::CreateMarket { market, rules_hash }),
            (market.clone(), state).prop_map(|(market, state)| Command::SetState { market, state }),
            (
                account.clone(),
                market.clone(),
                side,
                any::<u8>(),
                any::<u32>(),
                tif,
                any::<bool>(),
                prop::option::of(any::<u64>())
            )
                .prop_map(
                    |(account, market, side, price, qty, tif, post_only, client_order_id)| {
                        Command::Place(NewOrder {
                            account,
                            market,
                            side,
                            price,
                            qty,
                            tif,
                            post_only,
                            client_order_id,
                        })
                    }
                ),
            (account.clone(), any::<u64>()).prop_map(|(account, o)| Command::Cancel {
                account,
                order: OrderId::new(o)
            }),
            (account.clone(), any::<u64>(), any::<u32>()).prop_map(|(account, o, qty)| {
                Command::Amend {
                    account,
                    order: OrderId::new(o),
                    qty,
                }
            }),
            account
                .clone()
                .prop_map(|account| Command::OpenAccount { account }),
            (market.clone(), outcome, digest()).prop_map(|(market, outcome, evidence_hash)| {
                Command::Settle {
                    market,
                    outcome,
                    evidence_hash,
                }
            }),
            (prop::option::of(account), prop::option::of(market))
                .prop_map(|(account, market)| Command::CancelAll { account, market }),
            Just(Command::HaltAll),
        ]
    }

    fn bytes(command: &Command) -> Vec<u8> {
        let mut out = Vec::new();
        encode(command, &mut out);
        out
    }

    proptest! {
        #[test]
        fn every_command_round_trips(c in command()) {
            prop_assert_eq!(decode(&bytes(&c), FORMAT_VERSION), Ok(c));
        }

        #[test]
        fn every_strict_prefix_is_truncated(c in command()) {
            let full = bytes(&c);
            for len in 0..full.len() {
                prop_assert_eq!(decode(full.get(..len).unwrap(), FORMAT_VERSION), Err(CodecError::Truncated));
            }
        }

        #[test]
        fn a_trailing_byte_is_rejected(c in command(), extra in any::<u8>()) {
            let mut b = bytes(&c);
            b.push(extra);
            prop_assert_eq!(decode(&b, FORMAT_VERSION), Err(CodecError::TrailingBytes));
        }
    }

    #[test]
    fn unknown_tags_and_codes_are_rejected() {
        assert_eq!(decode(&[0], FORMAT_VERSION), Err(CodecError::UnknownTag(0)));
        assert_eq!(
            decode(&[10], FORMAT_VERSION),
            Err(CodecError::UnknownTag(10))
        );
        let mut set = bytes(&Command::SetState {
            market: MarketId::new(1),
            state: MarketState::Open,
        });
        *set.last_mut().unwrap() = 6;
        assert_eq!(
            decode(&set, FORMAT_VERSION),
            Err(CodecError::BadCode("state"))
        );
        let mut all = bytes(&Command::CancelAll {
            account: None,
            market: None,
        });
        *all.get_mut(1).unwrap() = 2;
        assert_eq!(
            decode(&all, FORMAT_VERSION),
            Err(CodecError::BadCode("account presence"))
        );
    }

    #[test]
    fn the_encoding_is_fixed_byte_for_byte() {
        // Pinned bytes: any change to the on-disk format must bump the segment version
        // (ADR-0006), and this test makes such a change impossible to miss.
        let place = Command::Place(NewOrder {
            account: AccountId::new(7),
            market: MarketId::new(2),
            side: Side::Sell,
            price: 45,
            qty: 300,
            tif: TimeInForce::Ioc,
            post_only: false,
            client_order_id: None,
        });
        // Version 2: the version 1 bytes, then a client order ID presence byte.
        let v1: [u8; 21] = [
            3, 7, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 45, 44, 1, 0, 0, 1, 0,
        ];
        let mut v2 = v1.to_vec();
        v2.push(0);
        assert_eq!(bytes(&place), v2);
        let Command::Place(mut with_id) = place else {
            unreachable!()
        };
        with_id.client_order_id = Some(0x0102);
        let mut v2_id = v1.to_vec();
        v2_id.extend_from_slice(&[1, 2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes(&Command::Place(with_id)), v2_id);
        assert_eq!(bytes(&Command::HaltAll), [9]);
        // Version 1 records still decode, as orders without a client order ID.
        assert_eq!(decode(&v1, 1), Ok(place));
        assert_eq!(decode(&v2, 1), Err(CodecError::TrailingBytes));
    }

    #[test]
    fn version_3_adds_the_rules_and_evidence_hashes() {
        let hash = Digest::new([0xab; 32]);
        let create = Command::CreateMarket {
            market: MarketId::new(0x0102),
            rules_hash: Some(hash),
        };
        let mut v3 = vec![1, 2, 1, 0, 0, 1];
        v3.extend_from_slice(&[0xab; 32]);
        assert_eq!(bytes(&create), v3);
        let settle = Command::Settle {
            market: MarketId::new(5),
            outcome: Outcome::Void,
            evidence_hash: None,
        };
        assert_eq!(bytes(&settle), [7, 5, 0, 0, 0, 2, 0]);
        // Versions 1 and 2 have no hash field: their records decode without one, and a
        // presence byte is a trailing byte.
        let old_create = [1, 2, 1, 0, 0];
        for version in [1, 2] {
            assert_eq!(
                decode(&old_create, version),
                Ok(Command::CreateMarket {
                    market: MarketId::new(0x0102),
                    rules_hash: None,
                })
            );
            assert_eq!(decode(&[7, 5, 0, 0, 0, 2], version), Ok(settle));
            assert_eq!(decode(&v3, version), Err(CodecError::TrailingBytes));
        }
        let mut bad = v3.clone();
        *bad.get_mut(5).unwrap() = 2;
        assert_eq!(
            decode(&bad, FORMAT_VERSION),
            Err(CodecError::BadCode("rules hash presence"))
        );
    }
}
