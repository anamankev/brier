# G1 review: matching engine

Closed at tag `gate-1` (`85d3825`).

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| 10 million random commands with no divergence from the reference engine | MET | 10,000,000 commands, 12,385,249 events, 716,607 trades, 0 divergences (`docs/notes/differential.md`) |
| P1 and P2 measured | MET | 12.27 M commands/s median; p99 334 ns, p99.9 500 ns. Both registered before the engine existed (`docs/notes/performance.md`) |

## What was built
- `brier-engine`: the book (two 99-level arrays of FIFO queues in an arena, ADR-0003), GTC, IOC, FOK, post-only, self-trade prevention, amend-down, the market lifecycle, and a BLAKE3 state hash.
- 15 rule tests, plus property tests over random streams (never crossed, consistent links, quantity conservation, deterministic replay).
- `brier-replay` (wire format in, events out) and the Python reference engine, written from the rules text.
- The differential harness and its CI job; `brier-bench`.

## Found in review
- The invariant checker iterated a hash map, so with two simultaneous failures, which one it reported could vary between runs. It now checks in sorted order.
- The first 10-million run stalled: the reference engine's cancel was O(markets). It now has an order-to-market index.

## Found in the post-G2 audit (fixed before G3)
- The G1 stream was weak: on a long run most cancels named dead orders and many orders targeted closed markets. Agreement on rejections still counts, but trading coverage was thinner than the event count suggests. The stream is now state-aware; see the G2 review and `docs/notes/differential.md`.
- The G1 rules lived only in `docs/gates/G1-plan.md`, which G2 later contradicted on one transition. `docs/protocol.md` is now the single current statement.

## Exam questions (asked 2026-11-06)
1. FOK counts liquidity only up to the trader's own first resting order. Construct a book where counting past it would accept an FOK that then cannot fill, and say which engine error would fire.
2. Amending down keeps time priority, but amending up loses it. Why would letting an increase keep its place be unfair, and who would exploit it?
3. Both engines came from the same plan text, by the same author. What kind of bug can the 10-million comparison never catch, and which tests guard against it?
4. The worst single command took 12.6 ms while 99.9% finished under 500 ns. Is that the engine's fault, and how would you prove it either way?
