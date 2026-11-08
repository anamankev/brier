# G7 review: reference market maker and simulation harness

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| The market maker quotes through the public API in simulation and on live markets | MET | In simulation: 800 seeded markets (`brier_services.sim`), using the same quoting code as the service. On live markets: the service under its own account and signed key, through the public REST API, on the launch catalogue on a real gateway, for 10,080 simulated minutes (`live_study.py`) |
| Metrics reported | MET | `docs/notes/market-maker.md` §4: P&L with t-statistics and intervals, inventory, spread, and time quoting in four simulated conditions; live uptime per market |

## Registered study: all four hypotheses passed
- **M1:** against noise traders only, mean settled P&L per market was +69.73 credits (t = 12.88, critical value 1.6526).
- **M2:** P&L fell as informed traders' signal improved: 80.25, then 71.78, then 54.77 credits.
- **M3:** the largest inventory was 518 contracts, within the bound of 525.
- **M4:** live uptime was 100% on all four modelled markets. As registered, this measures availability only: no other traders were present, and fair values did not move during the run.

## What was built
- **Quoting** (`marketmaker/quoting.py`): Avellaneda-Stoikov-shaped quotes in probability space:
  - an inventory skew on the reservation price, scaled by the outcome's variance and the time left;
  - a spread that widens with uncertainty and time, with a 2-tick floor;
  - only the reducing side quoted at the position limit.

  Property-tested, and checked against a worked example.
- **Fair values** (`marketmaker/fairvalue.py`), each matching its rule's rounding:
  - the 10-year yield as a daily random walk;
  - CPI by Monte Carlo over calendar-month log changes (2020-21 excluded);
  - unemployment as a random walk with a robust σ;
  - Central Park as 30-year climatology with ±7 days.

  The FOMC market is deliberately left unquoted: no free, licensed rate-expectations data exists.
- **The service** (`python -m brier_services.marketmaker run`):
  - Signed public API.
  - Fair-value inputs from BLS, FRED, and NOAA, with a refresh interval and a staleness limit.
  - Quotes kept rather than churned.
  - Quotes pulled when stale, when the market is halted or closed, or near the close.
  - One metrics line per market per pass.
- **The simulation harness** (`brier_services.sim`): seeded markets on in-process copies of the reference engine. Each has a hidden log-odds truth, noise and informed traders, and the maker. It is reproducible by seed.

## Found during G7, and fixed
1. **The live study ran into the gateway's per-address rate limit** (500 requests a second) on its first attempt and stopped with a 429. The runner now paces itself and retries a 429, as any client should; the rerun needed no retries. The limit did its job.
2. **A climatology test was wrong, not the model.** A ±7-day window around 25 December includes 1 January. The test date moved; the model is unchanged.

## Deviations and gaps
- **Where the statistics come from.** M1-M3 ran on the Python reference engine in-process, not through the gateway; only M4 used the gateway. The reference engine matches the Rust engine event for event (G1-G4 differential tests), but gateway-side effects (rate limits, latency) are absent from M1-M3.
- **No historical backtest yet.** The G7 plan's backtest of the fair-value models against past outcomes was not done, so the models' calibration on real history is unmeasured. This is the main gap.
- **The live run proves availability, not skill.** The maker was alone, and fair values did not change.
- **The position limit is the maker's own.** The exchange has no per-account position cap (threat model, G7).

## Compared with Kalshi and Polymarket
- **Behind:** both venues have many professional market makers, often with incentive programmes, quoting thousands of markets with models fed by paid data. Brier has one reference maker on four markets, with simple public-data models and no quote on the Fed market.
- **Ahead, as engineering evidence:** its behaviour is published and tested. Registered hypotheses, seeded simulations, and live uptime measured from the exchange's own book show what it does and where it stops.

## Exam questions
1. The maker's fair value lags the truth by 30 steps in simulation, and its P&L is still positive against noise. Derive roughly how large the lag (or the truth's volatility) would have to be before the spread no longer covers trend losses, using the registered parameters.
2. Poor informed traders raised the maker's P&L above the noise-only case. Explain why, and design the condition that would isolate adverse selection from extra order flow.
3. The exchange enforces collateral but not a position cap. What could a stolen maker key do in the worst case, and which exchange-side control would bound it without special-casing market makers?
4. The FOMC market has no quotes. Make the case for and against quoting it from 3-month Treasury bills, and say what evidence would change your answer.
