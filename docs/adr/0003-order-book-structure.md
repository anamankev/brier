# ADR 0003: Order book structure

Date: 2026-11-06  Status: Accepted

## Context
Every command on the order path touches the book: find the best opposite price, take the order at the front of a level, insert at the back, or remove an order from anywhere in its queue (cancel). Cancels are frequent (35% of the registered workload, and often more in real markets, where market makers re-quote constantly). Binary contracts trade only at the 99 integer ticks from 1 to 99 (SPEC §2.1). The engine must be deterministic (SPEC §3.2) and must not allocate per command in the steady state if it is to meet P1/P2.

## Options considered
1. **A sorted tree of price levels (`BTreeMap<Price, VecDeque<Order>>`)**, as general-purpose engines with unbounded price grids use. Best price is O(log L); a cancel needs a search inside the level's queue (O(n) in that level) or a tombstone scheme. Node allocation happens as levels appear and disappear.
2. **Two fixed arrays of 99 levels, each level a doubly linked FIFO threaded through an order arena with a free list, plus an order-ID map.** Best price is cached and, after a level empties, rescanned over at most 98 levels. Insert at the back, match from the front, and cancel anywhere are O(1) link updates. Arena slots are reused, so the steady state allocates nothing beyond the arena's high-water mark.
3. **A heap per side with lazy deletion.** Fast best price, but cancels leave stale entries that must be skipped, and iterating the book in priority order (state hash, market data snapshots) needs a sort.

## Decision
Option 2. With only 99 possible prices, a fixed array indexed by price is both simpler and faster than any tree. Index 0 is never used, so the index is the price itself, with no subtraction to get wrong. The doubly linked queue makes cancels O(1), which matters most for market-maker traffic. The order-ID map is used only for lookups and never iterated, so its hash order cannot leak into output.

## Consequences
- Easier: constant-time operations; priority-order iteration is a direct walk (used by the state hash, `final_book`, and later the market-data snapshot); invariants are cheap to check (`Book::check`).
- Harder: linked structures in an arena need careful link maintenance. This is covered by `Book::check` after every command in the property tests and by the differential tests against the list-based reference engine.
- The design depends on the bounded price grid. A product with a finer tick or an unbounded price would need option 1 instead.
- Wrong-choice signal: if profiling (performance note) shows best-price rescans or hash-map lookups dominating, revisit with a bitmap of non-empty levels or an intrusive ID index.
