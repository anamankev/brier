# Market maker: does it make money from noise, lose to information, and stay inside its limits? (G7)

Status: REGISTERED. Written before any market-maker code exists. The registering commit is the first commit that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

## 1. Questions
1. Does the reference market maker earn the spread from uninformed flow?
2. Does it lose more as the traders it faces know more (adverse selection), as theory says it must?
3. Does its inventory control hold?
4. On the real launch markets, does it keep a two-sided market whenever it should?

## 2. Hypotheses
- **M1 (noise).** Against noise traders only, mean settled P&L per market over 200 markets is greater than zero (one-sided t-test, α = 0.05). Refuted otherwise.
- **M2 (adverse selection).** With informed traders added at three signal qualities, mean settled P&L per market strictly decreases as signal quality improves. Refuted by any non-decrease.
- **M3 (limits).** In every simulated market, at every step, |inventory| ≤ the position limit plus one quote size. Refuted by a single breach.
- **M4 (uptime).** On a local exchange running the launch catalogue (all markets except the FOMC market, which has no model; G7 plan) across seven simulated days on the test clock, the market maker shows a two-sided quote in at least 95% of the minutes in which it should quote. It should quote when the market is open, more than 15 minutes from its close, and its fair value is fresh. Refuted below 95%.

## 3. Design
### Where it runs
- **M1-M3** run in the simulation harness against the Python reference engine, in-process. That engine is held to the Rust engine event for event by the differential tests (20 million commands, 0 divergences). Running in-process makes 1,200 markets feasible.
- The market maker's quoting code is the same code that runs against the gateway; only the transport differs.
- **M4** runs the real service against a real gateway (the test-clock build) through the public API.

### Simulated markets (M1-M3)
- **Markets and steps:** 200 per condition, seeds 1-200 (the same seeds in every condition), 480 one-minute steps each.
- **Hidden truth:** logit(p₀) ~ N(0, 1); each step adds N(0, 0.08²) to the logit. The outcome is YES with probability p at the last step.
- **Noise traders:** at each step, one arrives with probability 0.5. It buys or sells with equal chance, 1-20 contracts (uniform), immediate-or-cancel at the best opposite price.
- **Informed traders** (M2 only): at each step, one arrives with probability 0.15. It sees s = logit(p) + N(0, σ_s²) and turns it into a probability. It buys if that probability exceeds the best ask by 0.02, sells if it is below the best bid by 0.02, otherwise does nothing; 1-20 contracts, immediate-or-cancel. Signal qualities: σ_s = 2.0 (poor), 1.0, and 0.25 (good).
- **Market maker:** its fair value is the hidden p as of 30 steps earlier (public information that lags). It quotes 25 contracts a side with a position limit of 500 and requotes every step.
- **Quoting parameters, fixed here:**
  - **Skew:** fair p, normalized inventory q̂ = q / 500, σ² = p(1 − p), and τ the remaining fraction of the market's life. Reservation r = p − 0.08 · q̂ · (σ² / 0.25) · τ.
  - **Half-spread** = max(0.02, 0.015 + 0.02 · (σ² / 0.25) · τ).
  - **Ticks:** bid = ⌊100(r − half)⌋ and ask = ⌈100(r + half)⌉, held inside 1-99 with bid < ask.
  - **At the limit:** only the reducing side is quoted.
  - **Near the close:** quotes are pulled in the last 15 steps.
- **P&L per market:** cash change plus the settlement payout, in credits.

### Live run (M4)
- Markets 101, 102, 104, and 105 of `markets/`, each with its fair-value model from the G7 plan.
- Inputs are refreshed from the sources at the start; each model has a staleness limit.
- Seven simulated days on the test clock, with the market maker's loop running each simulated minute.
- Uptime is computed from the exchange's own book (best bid and best ask both present and owned by the maker), sampled each minute. It is not taken from the maker's own log.

### Reporting
Means, standard deviations, t-statistics, and 95% intervals for M1 and M2; the largest inventory seen for M3; uptime per market for M4. Also reported: average quoted spread, the share of time quoting, and the inventory path for one example market. Results go in §4 with the code commit and the machine.

## 4. Results
### M1-M3 (simulation)
Run on 8 November 2026 with code `db5b2f7` (`python -m brier_services.sim.study`), 800 markets in 5.7 s on the M5 MacBook Air.

| Condition | Mean P&L per market (credits) | SD | t | 95% interval | Max inventory | Average spread (ticks) | Time quoting |
|---|---|---|---|---|---|---|---|
| noise | 69.73 | 76.57 | 12.88 | 59.05 to 80.40 | 510 | 5.66 | 96.9% |
| informed sd 2.0 | 80.25 | 84.05 | 13.50 | 68.53 to 91.97 | 517 | 5.66 | 96.8% |
| informed sd 1.0 | 71.78 | 80.28 | 12.64 | 60.58 to 82.97 | 517 | 5.66 | 96.8% |
| informed sd 0.25 | 54.77 | 78.27 | 9.90 | 43.85 to 65.68 | 518 | 5.66 | 96.8% |

- **M1 passed:** t = 12.88 against the critical value 1.6526 (one-sided, 5%, 199 degrees of freedom).
- **M2 passed:** mean P&L falls as the informed traders' signal improves: 80.25, then 71.78, then 54.77 credits.
- **M3 passed:** the largest inventory in any market at any step was 518 contracts, within the bound of 525 (limit plus one quote size).

Notes:
- **Poor informed traders raised the maker's P&L above noise alone** (80.25 against 69.73 credits). With a signal that weak, the "informed" traders are mostly extra noise flow paying the spread. M2 compares only the three informed levels, as registered.
- **The P&L includes inventory gains and losses, not only the spread.** The maker's fair value lags the truth by 30 steps, so trends cost it; the spread more than covers that against noise.
- **Time quoting is 96.8-96.9%** because quotes are pulled for the last 15 of 480 steps (3.1%).
- **These are simulated markets.** They show the quoting logic behaves as theory says; they say nothing about real traders, who are not drawn from these distributions.

### M4 (live)
Not yet run.
