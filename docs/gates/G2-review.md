# G2 review: ledger and risk

Closed at tag `gate-2` (`3721932`). The post-G2 audit (`18e9229` to `f93040d`) strengthened the evidence below before G3 began.

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| I1-I5 hold over 10 million random commands | MET | State-aware 10-million run: 19,596,192 events, 696,614 trades, 0 divergences, full I1-I5 checks in both engines every 100,000 commands (`docs/notes/differential.md`) |
| Settlement zeroes escrow | MET | `settlement_always_zeroes_escrow` property; the engine halts if escrow is nonzero after any settlement |
| The play-money test (SPEC §1.2) passes | MET | `every_entry_is_a_grant_a_trade_or_a_settlement` |

## What was built
- A double-entry ledger with three entry kinds (grant, trade, settle) and collateral as a lock on cash, with the worst-case rule and its proof (ADR-0005).
- Settlement: YES, NO, and void at 50 (ADR-0002). Accounts with a signup grant, the resting-order limit, and kill switches (`cancel_all`, `halt_all`).
- 14 money rule tests; two new properties; the reference engine and replay tool extended; periodic full invariant checks in long runs (`--check-every`).

## Found in the post-G2 audit, and fixed
1. A failed command did not halt the engine for good, so a caller could keep going on half-applied state. Now every later command is refused with `Halted`, and `halted()` names the failing command.
2. Settled markets kept their per-account state forever. Now it is freed, after a check that it is empty.
3. The rules were split across two plan documents, one of them stale. `docs/protocol.md` is now the single current statement.
4. The SPEC overstated where invariants are checked, and claimed periodic state hashes the engine does not emit. It now states exactly what exists.
5. The differential streams were mostly rejections (86%). The state-aware generator, the live-market cap, and liveness floors in the fast test fix this.
6. The benchmark hid its rejection share. It now reports its measured mix.
7. `make bench` and `make audit` failed on a fresh clone or off macOS. Fixed.

## Exam questions
1. ADR-0005 proves `cash ≥ reserved` survives any fill. Walk through the proof for a seller who holds 10 YES, rests a sell of 15 at 60, and is filled for 12. Show the reserve before and after, and the cash received.
2. Why does a void settle at 50 per contract instead of refunding each account's net cost? Construct an account that a refund would push to negative cash.
3. The collateral check counts an IOC order as if it rests in full, although it never rests. Why is that still the right check, and what would go wrong if it counted only the IOC's crossing quantity at today's book?
4. The 86%-rejection stream agreed on every event. What class of bug in the money code could it have missed, and what makes the state-aware stream catch it?
