# ADR 0005: Collateral is a lock on cash, computed by a worst-case rule

Date: 2026-11-06  Status: Accepted

## Context
Contracts are fully collateralized (SPEC §2): an order is accepted only if its owner can pay for every unit it could still fill. Resting orders can fill later, in any order, against any counterparty, and some fills close positions (releasing cash) while others open them (needing cash). The accounting must never let cash go negative (I3), and must not over-reserve an account that is selling contracts it already holds.

## Options considered
1. **Separate `available` and `reserved` ledger accounts, with Reserve and Release entries.** Trades would have to decide which pool pays, and a fill could transiently leave one pool negative. That gives more entry types and more room for error.
2. **Reserve the full price of every order, ignoring holdings.** Simple and safe, but an account holding 100 NO contracts would need cash to buy YES, even though closing pays it 100 − p per unit.
3. **One cash balance per account; collateral is a lock.** `reserved(a, m)` is recomputed from the account's resting orders and position by a worst-case rule. `available = cash − reserved`, and trades move cash directly.

## Decision
Option 3, with this rule (`docs/gates/G2-plan.md`):

- `reserve_buy`: with C = max(0, −x) NO contracts held and Q resting buy units, the sum of the prices of the Q − C most expensive units.
- `reserve_sell`: with C = max(0, x) YES contracts held, the sum of (100 − p) over the Q − C most expensive sell units, which are the lowest-priced ones.
- `reserve = reserve_buy + reserve_sell`.

**Sufficiency.** Take one side; the other is symmetric. Before a fill of k buy units at their limit prices (a taker may fill cheaper, which only helps), suppose position x < 0 with C = |x|.

- If k ≤ C, all k units close and the account *receives* cash. The new reserve sums the top Q − C of fewer units, which is no more than before.
- If k > C, C units close (cash in) and k − C open at their prices. The opened units together with the new top Q − k units form a set of Q − C units, so their sum is at most the old top Q − C sum. That is, realized cost + new reserve ≤ old reserve.

Fills on the opposite side only raise C on this side (selling YES while short adds NO contracts) or leave it unchanged, so they cannot raise this side's reserve. By induction over fills, if `cash ≥ reserved` held before a command, and the incoming order passed the check with itself counted as resting, it holds after.

## Consequences
- The ledger has three entry types (Grant, Trade, Settle). Reservations are reported as `reserved` events.
- Recomputing a reserve walks 99 price levels per side for one (account, market): O(1), using per-price quantity arrays.
- `cash ≥ reserved` is checked after every command in tests (I3), and property tests exercise the proof's cases with random streams.
- Wrong-choice signal: if an invariant check ever finds cash below reserved, the proof or its implementation is wrong. The engine halts rather than adjusting anything.
