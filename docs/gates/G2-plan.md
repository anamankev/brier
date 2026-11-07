# G2 plan: ledger and risk

Reads: SPEC §2.3, §2.4, §4. Exit: invariants I1-I5 hold over 10 million random commands; settlement zeroes escrow; the play-money test (§1.2) passes.

## Money model (ADR-0005)
- Each account has one ledger balance, `cash` (credit-cents). Collateral is a **lock** on cash, not a separate ledger account: `reserved(a)` is the sum over markets of `reserve(a, m)`, and `available(a) = cash(a) − reserved(a)`.
- Ledger accounts: `cash:<account>`, `escrow:<market>`, `issuance`. Every entry's legs sum to zero.
- Entry kinds, and the only ones that exist:
  - `grant`: issuance → one account's cash.
  - `trade`: the buyer's cash, the seller's cash, and the market's escrow.
  - `settle`: escrow → one account's cash.

## Positions and trade cash (SPEC §2.3)
- Position x(a, m) is signed: x > 0 means YES contracts, x < 0 means NO contracts.
- For a trade of f contracts at price p between buyer b and seller s (YES terms), with positions before the trade:
  - buyer closing units `cb = min(f, max(0, −x_b))`; seller opening units `os = f − min(f, max(0, x_s))`.
  - Buyer cash: `−p·f + 100·cb`. Seller cash: `+p·f − 100·os`. Escrow: `+100·os − 100·cb`.
  - Positions: `x_b += f`, `x_s −= f`.
- Checks: the legs sum to zero; escrow stays equal to 100 × open interest (I2).

## Collateral (SPEC §4.4)
- Per (account, market), track resting quantity by side and price: `buy_qty[p]` and `sell_qty[p]`.
- `reserve(a, m) = reserve_buy + reserve_sell`:
  - `reserve_buy`: let C = max(0, −x) be the NO contracts held, and Q the total resting buy quantity. It is the sum of the prices of the **Q − C most expensive** resting buy units. Units that close NO need no cash, and assuming the cheap units close is the worst case.
  - `reserve_sell`: let C = max(0, x) be the YES contracts held. Each opening sell unit at price p costs 100 − p. It is the sum of (100 − p) over the **Q − C most expensive** resting sell units, which are the lowest-priced ones.
- Why this is enough (proof in ADR-0005): for any fill, the realized cash cost plus the new reserve is at most the old reserve. So `cash ≥ reserved` holds after every command if it held before and the incoming order passed the check.
- Pre-trade check: with the incoming order counted as if it rests in full, `reserve(a, m)` must grow by no more than `available(a)`. Otherwise reject `insufficient_collateral`. This applies to IOC and FOK too: they may fill at any price up to their limit.
- At the end of every command, recompute `reserve` for every (account, market) the command touched. Emit `reserved` events for each pair whose reserve changed, sorted by (account, market).

## Settlement (SPEC §2.4, ADR-0002)
- New command `settle {market, outcome: yes | no | void}`, valid only when the market is Closed. Otherwise `invalid_transition`. It moves the market to Resolved (yes/no) or Voided.
- Payouts, in ascending account order, skipping zero amounts:
  - YES: 100 per YES contract.
  - NO: 100 per NO contract.
  - Void: 50 per contract on both sides.

  Each payout is a `settle` entry. Every position in the market becomes 0, and escrow must then be exactly 0.
- `set_state` can no longer target Resolved or Voided (`invalid_transition`); only `settle` reaches them.

## Accounts and limits
- New command `open_account {account}`: grants `SIGNUP_GRANT` = 1,000,000 credit-cents (10,000 credits) from issuance. A duplicate is rejected with `account_exists`.
- `MAX_RESTING_ORDERS` = 1,000 per account across all markets. It applies to GTC orders only, since IOC and FOK never rest. Over the limit: `too_many_orders`.

## Kill switches (SPEC §4.5)
- `cancel_all {account?, market?}`: cancels every matching resting order in ascending order ID, with reason `kill_switch`. With neither field, it cancels everything. An `account` or `market` that doesn't exist is rejected (`unknown_account`, `unknown_market`).
- `halt_all`: every Open market becomes Halted, in ascending market ID.

## Check order
- Place: `unknown_market`, `unknown_account`, `market_not_open`, `invalid_price`, `invalid_quantity`, `invalid_order`, `post_only_would_cross`, `fok_unfillable`, `too_many_orders`, `insufficient_collateral`.
- Cancel, amend: unchanged. An account that doesn't exist gets `unknown_order`, so nothing is revealed.
- Settle: `unknown_market`, `invalid_transition`.
- Cancel all: `unknown_account`, `unknown_market`.

## Wire additions
- Commands: `open_account`, `settle`, `cancel_all`, `halt_all`, with the fields above.
- Events:
  - `account_opened` (account, cash)
  - `entry` (kind, legs), where `legs` is a list of `[ledger_account, amount]`, nonzero amounts only, in this order: grant [issuance, cash]; trade [buyer cash, seller cash, escrow]; settle [escrow, cash]
  - `reserved` (account, market, amount)
  - `settled` (market, outcome)
- Within one command, events come in this order: the G1 events (with each trade's `entry` right after its `trade`), then `settled` and its payout entries, then the `reserved` events.
- `final_book` adds:
  - `accounts`: per account ascending, cash, reserved, and `positions` as `[market, x]` with x ≠ 0, in ascending market
  - `escrow`: `[market, amount]` per market, ascending

## Units
1. ADR-0002 (void at 50) and ADR-0005 (collateral as a lock, with the proof); SPEC §4 updated to match.
2. Rust: a `ledger` module (accounts, positions, escrow, entries, I1-I4 checks) and the `reserve` function, wired into the engine; settle, open account, cancel all, halt all.
3. Tests:
   - rule tests for every item above;
   - property tests: I1-I5 after every command, settlement zeroes escrow, entries balance, and the play-money check (every entry is one of the three kinds, with exactly the legs listed).
4. Replay tool and reference engine extended. The generator adds accounts, settlements, kill switches, and large orders, so `insufficient_collateral` fires.
5. 10 million commands through both engines, with every money event compared and the I1-I5 checks running.
6. Gate review.
