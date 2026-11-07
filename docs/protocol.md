# Brier engine protocol

The exact behavior of the engine: every command, the order of its checks, what it changes, and the events it emits, in the wire format shared by the Rust engine (`crates/engine`), the replay tool (`crates/replay`), and the Python reference engine (`services/src/brier_services/reference_engine.py`). Both engines implement this document, and the differential tests hold them to it event for event.

This document is current. The gate plans in `docs/gates/` record how each part was designed and are kept as history; where they differ from this document, this document is right. A change to engine behavior changes this document in the same commit.

## 1. Units and identifiers
- **Price:** integer ticks 1-99, the price of a YES contract. A NO contract's price is 100 − p. On the wire `price` is a u8 (0-255), and values outside 1-99 are rejected, never clipped.
- **Quantity:** whole contracts 1-25,000 (`MAX_ORDER_QTY`). On the wire `qty` is a u32.
- **Money:** integer credit-cents (i64). A winning contract pays 100.
- **Sequence:** every command carries `seq`. The first is 1, and each next one is exactly one more.
- **Order ID:** an accepted order's ID is the `seq` of the command that placed it.
- **Client order ID:** optional on `place`, a u64 chosen by the client (see §3).
- **Wire values outside their wire type** (a price of 300, a missing field, an unknown command) are malformed input. They stop the replay tool (exit 2) and are not rejections.

## 2. Halting
A sequence gap, or an internal invariant failing, halts the engine. A halted engine refuses every later command (`Halted`, naming the command that halted it), because the failing command may have changed state part-way. Recovery is a restart from the journal (G3).

## 3. Commands, checks, and effects

The first failing check names the rejection. A rejected command changes nothing.

| Command | Fields | Checks, in order | Effect |
|---|---|---|---|
| `open_account` | account | `account_exists` | Grants `SIGNUP_GRANT` = 1,000,000 from issuance |
| `create_market` | market | `market_exists` | Lists the market in `pre_open` with escrow 0 |
| `set_state` | market, state | `unknown_market`, `invalid_transition` | Changes state; entering `closed` cancels every resting order in the market |
| `place` | account, market, side, price, qty, tif, post_only, client_order_id? | `unknown_market`, `unknown_account`, then `duplicate` (not a rejection; below), `market_not_open`, `invalid_price`, `invalid_quantity`, `invalid_order`, `post_only_would_cross`, `fok_unfillable`, `too_many_orders`, `insufficient_collateral` | Matches, then rests (GTC) or cancels the remainder (IOC) |
| `cancel` | account, order | `unknown_order` | Cancels the order |
| `amend` | account, order, qty | `unknown_order`, `market_not_open`, `invalid_quantity`, `invalid_amend` | Reduces the remaining quantity; keeps time priority |
| `settle` | market, outcome (`yes`, `no`, `void`) | `unknown_market`, `invalid_transition` (the market must be `closed`) | Pays out every position; the market becomes `resolved` or `voided` |
| `cancel_all` | account?, market? | `unknown_account`, `unknown_market` | Cancels every matching resting order; with neither field, all of them |
| `halt_all` | none | none | Every `open` market becomes `halted` |

Notes on the checks:
- `unknown_order` covers an order that never existed, has already gone, or belongs to another account, so other accounts' orders are never revealed. Cancel and amend from an account that doesn't exist also answer `unknown_order`.
- `invalid_order`: `post_only` without `gtc`.
- `invalid_amend`: the new quantity is not below the remaining quantity.
- `too_many_orders`: GTC only, when the account already has 1,000 (`MAX_RESTING_ORDERS`) resting orders across all markets.
- `insufficient_collateral`: see §5.
- **Client order IDs (idempotency):** if the account already has an accepted order with this `client_order_id` among its last 10,000 accepted IDs (`CLIENT_ID_WINDOW`, oldest evicted first), the command emits a single `duplicate` event naming the original order and changes nothing else. An ID is recorded only when its order is accepted, so a rejected order can be retried with the same ID. The windows are engine state: in snapshots, and rebuilt by replay.

### Market states
`pre_open` → `open` or `closed`; `open` → `halted` or `closed`; `halted` → `open` or `closed`. Any other `set_state`, including one to the current state, is `invalid_transition`. `resolved` and `voided` are reached only through `settle`. Place and amend need `open`; cancel works in `pre_open`, `open`, and `halted`.

## 4. Matching
- One book per market, quoted in YES terms. Sides are `buy` and `sell` of YES.
- Price-time priority: the best price first (highest bid, lowest ask), and the oldest order (lowest ID) first within a price. A trade executes at the resting order's price.
- **GTC:** match, then rest any remainder. **IOC:** match, then cancel any remainder (`ioc_remainder`), including when nothing crossed. **FOK:** before matching, sum the resting quantity that would fill, walking the opposite side in priority order. Stop at the limit price or at the first order owned by the same account. If the sum is short, reject `fok_unfillable` without trading.
- **Post-only:** rejected if it would cross on arrival.
- **Self-trade prevention (cancel taker):** if the next resting order to match belongs to the incoming order's account, matching stops and the incoming remainder is cancelled (`self_trade`). Trades already made stand, and the resting order is untouched.
- Fully filled resting orders leave the book; the `trade` event records the fill.

## 5. Money (SPEC §2.3-§2.4, §4; ADR-0002, ADR-0005)
- **Balances:** each account has `cash`. Collateral is a lock: `reserved(a)` is the sum over markets of `reserve(a, m)`, and `available = cash − reserved`. Escrow per market; issuance is minus every credit granted.
- **Positions:** x(a, m) > 0 is YES contracts, < 0 is NO contracts.
- **Trade cash,** for f contracts at price p between buyer b and seller s, using positions before the trade:
  - buyer closing units `cb = min(f, max(0, −x_b))`; seller opening units `os = f − min(f, max(0, x_s))`.
  - Buyer cash `−p·f + 100·cb`; seller cash `+p·f − 100·os`; escrow `+100·os − 100·cb`.
  - Positions: `x_b += f`, `x_s −= f`.
- **Reserve:** `reserve(a, m)` is the sum of two parts:
  - **Buy units:** the price of each resting buy unit, except that the account's NO contracts cover its cheapest buy units for free.
  - **Sell units:** (100 − price) for each resting sell unit, except that its YES contracts cover its cheapest sell units (the highest-priced ones) for free.
- **Collateral check:** count the incoming order as if it rests in full. `reserve(a, m)` may grow by at most `available(a)`, or the order is rejected `insufficient_collateral`. IOC and FOK orders are checked the same way.
- **Settlement:**
  - YES pays 100 per YES contract; NO pays 100 per NO contract; void pays 50 per contract on both sides.
  - Payouts go in ascending account order, skipping zero amounts. Afterwards every position in the market is zero, escrow is exactly zero, and the market's per-account state is freed.
- **Invariants (SPEC §4.3):**
  - I1: Σ cash + Σ escrow + issuance = 0.
  - I2: escrow = 100 × open interest.
  - I3: cash ≥ reserved ≥ 0.
  - I4: reserves match their definition.
  - I5: the book is never crossed and its structure is consistent.

## 6. Events

Every event carries the `seq` of its command and an `ev` field.

| `ev` | Fields |
|---|---|
| `account_opened` | account, cash |
| `market_created` | market |
| `market_state` | market, state |
| `accepted` | order |
| `trade` | market, maker, taker, maker_account, taker_account, taker_side, price, qty |
| `entry` | kind (`grant`, `trade`, `settle`), legs |
| `rested` | order, remaining |
| `amended` | order, remaining |
| `cancelled` | order, remaining, reason (`user`, `ioc_remainder`, `self_trade`, `market_closed`, `kill_switch`) |
| `rejected` | reason |
| `duplicate` | order (the original order with that client order ID) |
| `settled` | market, outcome |
| `reserved` | account, market, amount (the new reserve) |

- **Ledger entries:** `legs` is a list of `[ledger_account, amount]`, with ledger accounts `cash:<id>`, `escrow:<id>`, and `issuance`. Zero legs are left out and the legs sum to zero. Leg order: grant `[issuance, cash]`; trade `[buyer cash, seller cash, escrow]`; settle `[escrow, cash]`. These are the only three entry kinds.
- **Order within one command:**
  1. The command's own events: for `open_account`, `account_opened` then its grant `entry`. For `place`, `accepted`, then each `trade` immediately followed by its `entry`, then at most one `rested` or `cancelled`. For `set_state` to `closed`, `market_state` then the cancellations in ascending order ID. For `cancel_all`, the cancellations in ascending order ID. For `settle`, `settled` then the payout entries.
  2. Then `reserved` for every (account, market) whose reserve changed, sorted by account and then market.

## 7. End-of-input record (replay tool and reference engine)
One `final_book` record:
- `markets`: per market in ascending ID, its `state`, `bids` and `asks`, each best price first, as `[order, account, price, remaining]` in time priority.
- `accounts`: per account in ascending ID, `cash`, `reserved`, and `positions` as `[market, x]` for x ≠ 0, in ascending market.
- `escrow`: `[market, amount]` per market, ascending.

## 8. Not yet in the protocol
- Timestamps on commands: the sequencer assigns them in G3/G4, and the engine will only echo them.
- Snapshot and journal records (G3); API envelopes, authentication, and client order IDs (G4).
