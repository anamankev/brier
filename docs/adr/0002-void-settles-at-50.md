# ADR 0002: A voided market settles every contract at 50

Date: 2026-11-06  Status: Accepted

## Context
Some markets cannot resolve as their rulebook intends: the source never publishes, the question turns out ambiguous, or an error is found in the listing (SPEC §7). The exchange needs a settlement that is fair to both sides, pays out exactly the escrow it holds, and never needs money back from anyone.

## Options considered
1. **Refund each account's net cost in the market.** Every account gets back what it paid in, minus what it took out. This undoes the market, but an account that bought at 30 and sold at 60 has a negative net cost, so it would owe credits. That is a clawback, which the ledger cannot do without driving cash negative (I3).
2. **Cancel every trade.** Equivalent to option 1, with the same clawback.
3. **Settle every contract on both sides at 50.** Each YES and each NO contract pays 50. Every pair is backed by 100 in escrow, so payouts total 50 × 2 × open interest = escrow exactly. Nobody pays anything back; realized gains and losses from trades already done stand.

## Decision
Option 3. It is symmetric (neither side is favored), it zeroes escrow by construction (I2), and it never needs a clawback. Prediction-market venues use the same convention for unresolvable markets.

## Consequences
- An account that bought YES at 90 and holds it through a void loses 40 per contract; one that bought at 10 gains 40. The rulebook must state the void conditions up front (SPEC §7), so traders can price that risk.
- Tests: after any void, escrow is 0 and the sum of payouts equals the escrow before settlement.
- Wrong-choice signal: if voids become frequent, the 50/50 convention would reward traders who bet on rule ambiguity rather than the event. Treat that as a rulebook-quality problem first.
