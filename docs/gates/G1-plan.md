# G1 plan: matching engine

Reads: SPEC §2, §3. Exit: 10 million random commands with no divergence from the reference engine; P1 and P2 measured.

Collateral, positions, the resting-order limit, and kill switches are G2 (SPEC §4). The G1 engine matches orders and keeps the book. It does not yet check whether an account can afford an order.

## Semantics fixed for both engines (from SPEC §3.1, made precise here)
- **Order IDs:** an accepted order's ID equals the sequence number of the command that placed it.
- **Sequence:** commands must arrive with gap-free sequence numbers starting at 1. A gap is an engine error that halts the engine. It is not a rejection.
- **Event order within one command:** `accepted`, then each `trade` in match order, then at most one of `rested` or `cancelled` for the incoming order.
- **Rejections** change no state:
  - `unknown_market`, `market_exists`, `market_not_open` (place or amend when the state is not Open);
  - `invalid_transition`, `invalid_order` (post-only without GTC), `invalid_price`, `invalid_quantity` (wire-level values out of range);
  - `post_only_would_cross`, `fok_unfillable`;
  - `unknown_order` (missing, already gone, or owned by another account, so the existence of other users' orders is never revealed);
  - `invalid_amend` (new quantity not below the remaining quantity).
- **IOC:** fills what crosses; any remainder is `cancelled` with reason `ioc_remainder`.
- **FOK:** before matching, sum the resting quantity that would fill, walking the opposite side best price first and FIFO within a level. Stop at the limit price or at the first order owned by the same account (self-trade prevention would stop there). If the sum is below the order quantity, reject `fok_unfillable`; otherwise match.
- **Self-trade prevention (CancelTaker):** if the next resting order to match belongs to the incoming order's account, stop matching. Cancel the incoming remainder with reason `self_trade`. Trades already made stand.
- **Amend down:** allowed only while Open, to a quantity of at least 1 and below the remaining quantity. It keeps priority.
- **Cancel:** allowed in PreOpen, Open, and Halted. Reason `user`.
- **State transitions:** PreOpen→Open, PreOpen→Closed, Open→Halted, Halted→Open, Open→Closed, Halted→Closed, Closed→Resolved, Closed→Voided. Anything else, including a transition to the current state, is `invalid_transition`. Entering Closed cancels every resting order, in ascending order ID, with reason `market_closed`.
- **Fully filled makers** leave the book silently; the `trade` event carries the fill.

## Wire format (JSON lines; shared by the replay tool and the reference engine)
- Commands:
  - `{"seq", "cmd": "create_market", "market"}`
  - `{"seq", "cmd": "set_state", "market", "state"}`
  - `{"seq", "cmd": "place", "account", "market", "side", "price", "qty", "tif", "post_only"}`
  - `{"seq", "cmd": "cancel", "account", "order"}`
  - `{"seq", "cmd": "amend", "account", "order", "qty"}`
- Events: each event carries the `seq` of its command and an `ev` field:
  - `market_created`, `market_state`, `accepted`, `rested`, `amended`, `rejected`;
  - `trade`: maker, taker, maker_account, taker_account, taker_side, price, qty;
  - `cancelled`: order, remaining, reason.
- At end of input, a `final_book` record: per market in ascending ID, the state, bids from best price down, and asks from best price up, each level FIFO as `[order, account, remaining]`.

## Units
1. Register P1 and P2 in `docs/notes/performance.md` before measuring.
2. `crates/engine`. The book is two 99-level arrays of FIFO queues, linked through an order arena with a free list (SPEC §3.3). The engine also holds the order-ID map, the market-state machine, matching for all order types, and a BLAKE3 state hash. Unit tests cover every rule above.
3. Property tests over random command streams:
   - the book is never crossed;
   - every resting order has quantity > 0;
   - the arena, the level links, and the order-ID map agree;
   - each order's quantity is conserved (filled + cancelled + resting = original);
   - two runs of the same stream give identical events and state hashes.
4. `crates/replay`: a binary that reads wire commands on stdin and writes wire events on stdout. G3 reuses it for journal replay.
5. `services/.../reference_engine.py`: written from this plan and SPEC §2-§3, not from the Rust code. Plain lists, O(n) per operation, deliberately obvious. It has its own unit tests.
6. Differential harness: a seeded random stream generator that mixes valid and invalid commands and every order type. A fast pytest compares the two engines on several seeds; a slow run covers 10 million commands. A CI job builds the Rust binary and runs the fast comparison.
7. `crates/bench`: the P1/P2 workload with per-command timing into an HDR histogram. Results go in the performance note, with hardware and power state.
8. Gate review.
