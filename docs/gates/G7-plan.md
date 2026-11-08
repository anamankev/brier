# G7 plan: reference market maker and simulation harness

Reads: SPEC §8, §10, §6, §11. Exit: the market maker quotes through the public API in simulation and on live markets; its metrics are reported.

## The problem
An exchange with an empty book is not a market. On Kalshi and Polymarket, professional market makers supply most of the quotes. Brier needs one reference market maker that:
- trades only through the public API, with the same limits as any user;
- quotes around a fair value it can defend;
- reports how it did.

It also needs a seeded, reproducible simulated market to test against. G8 will use the same harness to inject manipulators.

## Fair value (SPEC §8: "a documented, simple model or no quotes at all")
Each model uses only the free public sources the resolvers already read (`docs/data-licenses.md`), and each gives P(the rule resolves YES) for the rulebook as written, including its rounding.

| Market type | Model | Inputs |
|---|---|---|
| 10-year yield above X on date D | The yield follows a random walk in daily steps: P = P(round(y + ε, 2) > X), ε ~ N(0, σ²·n), where n is the number of trading days to D and σ is the standard deviation of daily changes over the last 250 days | FRED `DGS10` |
| CPI 12-month change above X for month M | Each month not yet published adds a log change drawn from that calendar month's history (mean and standard deviation over 10 years, excluding 2020-21). The year-on-year change is computed exactly as the resolver does and rounded to one decimal; P comes from 20,000 seeded Monte Carlo draws | BLS `CUUR0000SA0` |
| Unemployment rate at or above X for month M | A random walk in monthly steps, σ estimated robustly (median absolute deviation × 1.4826) over 10 years so that April 2020 does not dominate; P(round(u, 1) ≥ X) | BLS `LNS14000000` |
| Central Park maximum at or above X on date D | Climatology: the share of the last 30 years in which TMAX on days within ±7 of D's calendar day reached X °F | NOAA GHCN-Daily by-station file |
| FOMC upper limit | **No quotes.** No free, licensed source of fed funds futures or OIS exists for Brier. A model built from Treasury bills would be guesswork dressed up as a price | — |

A model that cannot refresh its inputs (an HTTP error, stale data) marks fair value stale, and the market maker pulls its quotes.

## Quoting (Avellaneda and Stoikov 2008, in probability space)
- **Reservation price:** r = p − q·γ·σ²·τ, where p is the fair probability, q is the inventory (positive for net YES), σ² is p(1 − p), γ is risk aversion, and τ is the fraction of the market's life remaining.
- **Half-spread:** max(floor, ½·(γ·σ²·τ + (2/γ)·ln(1 + γ/κ))), with the floor at 2 ticks.
- **Quotes:** bid = ⌊100·(r − half)⌋ and ask = ⌈100·(r + half)⌉, held inside 1–99 with bid < ask, in a fixed size.
- **Position limit:** at the limit, quote only on the side that reduces the position.
- **Quotes are pulled when:**
  - fair value is stale (older than its source's refresh interval);
  - the market is not open;
  - the market is within 15 minutes of its close;
  - the exchange answers with errors.
- Orders carry client order IDs; the maker amends or cancels and replaces only when its price moves by at least one tick.

## The service (`python -m brier_services.marketmaker`)
- Signs its requests with its own key, through the public REST and WebSocket APIs.
- Uses no admin role and no special limits.
- Logs one JSON line per action.
- Writes its metrics to a file the study reads:
  - time quoting per market;
  - average quoted spread;
  - the inventory path;
  - P&L by market, both marked to fair value and settled.

## Simulation harness (`brier_services/sim/`, SPEC §10)
- Agent-based and fully seeded: the same seed gives the same orders.
- Runs against a real gateway (the test-clock build, as in the G5 exit test) in its own database schema, never alongside real users' markets.
- Each simulated market has a hidden true probability path, and its outcome is drawn from the path's end.
- **Agents:**
  - noise traders, with Poisson arrivals, random side, and limit prices near the mid;
  - informed traders, who see the true probability plus noise of chosen standard deviation (the "signal quality");
  - the reference market maker, given fair value equal to the public information (the true path with a lag);
  - one manipulator type per surveillance typology. These are built in G8, but the harness reserves their interface now.

## Registered study (`docs/notes/market-maker.md`, written before the market maker runs)
- **M1:** Against noise traders only, the market maker's settled P&L over 200 simulated markets is positive. One-sided t-test at 5%.
- **M2:** With informed traders added, the market maker's P&L decreases as signal quality improves (adverse selection). Measured at three signal levels; refuted if it does not fall monotonically.
- **M3:** Inventory never exceeds the position limit by more than one order's size. Refuted by one breach.
- **M4:** On the live launch markets (all except the FOMC market), on a local exchange running the test clock across one simulated week, the market maker quotes a two-sided market at least 95% of the time it should be quoting.
- The note fixes the parameters (γ, κ, sizes, limits), the seeds, and the market count before any run.

## Units
1. This plan and the registered note.
2. Fair-value models, tested on recorded responses, including the rounding of each rule. Sanity checks against history: forecasts for past months are scored by Brier score against what happened.
3. Quoting functions and property tests:
   - quotes stay within 1–99 with bid below ask;
   - quotes move against inventory;
   - the floor holds;
   - one side only at the limit.
4. The market maker service: signed key, the WebSocket book, amend and replace, pulling quotes, metrics.
5. The simulation harness: agents, the scenario runner, and a reproducibility test (same seed, same journal hash).
6. The study: M1-M3 in simulation, M4 on the live markets, and the results written up.
7. The G7 threat-model section (a rogue or compromised market-maker key) and the gate review.
