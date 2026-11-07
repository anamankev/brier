# G5 review: markets, rulebook, resolution

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| Every launch-catalogue market resolves end to end from a recorded source response | MET, with one qualification | `services/tests/test_launch_catalogue.py`: a real gateway process on a file clock lists all five markets in `markets/`, trades them, opens and closes them by schedule, resolves them through the resolver service (signed, resolver role), confirms them, rejects a dispute, voids the missing yield at its deadline, and settles. Checked: payouts, conserved credits, journaled rules and evidence hashes equal to the rulebooks and evidence records, offline recomputation from the stored bytes, and the read models. **Qualification:** the November and December values do not exist yet, so each response is the recorded one for that series with the period's value added, in the source's exact format; FRED's are hand-written (its terms). Real responses are tested by the R1 study below |
| Registered R1/R2 study (`docs/notes/resolution.md`) | PASSED on attempt 3 | R1: 855 instances over six types against independent comparators, 0 mismatches, nothing one-sided. R2: 855 decisions recomputed from stored bytes in a process with networking disabled, 0 differences. Attempts 1 and 2 are reported, with what each found |

## What was built
- **Rulebooks** (`docs/rulebook.md`). One TOML file per market: question, source, observation period, a mechanical condition (`value op threshold`), schedule, revision policy, void conditions.
  - The validator exists twice, in Rust (`crates/rules`, used by the gateway) and Python (`brier_services.rules`, used by the resolvers), held together by 39 shared test vectors and a shared series registry.
  - The rules hash is the SHA-256 of the rulebook's RFC 8785 canonical JSON.
  - Timing rules make trading on a known answer impossible: close before release, and close before the period starts for anything that can be watched live.
- **Rules and evidence in the journal.** Journal format v3: `create_market` carries the rules hash and `settle` the evidence hash. The engine passes both to its events and stores neither, so snapshots and the state hash did not change. Old journals still read. The reference engine, the differential generator, and the fuzz targets changed with it.
- **The resolution workflow** (`docs/resolution.md`), in the gateway and PostgreSQL.
  - A resolver role that can only propose.
  - Confirmation by a second admin, a 24 h dispute window open to position holders, and rulings.
  - A settler that claims a resolution before settling it and finishes after a crash from the journal.
  - A scheduler that opens and closes markets at their rulebook times. Every step is audited; the gateway's own commands as `system`.
  - The one-step settle endpoint is gone.
- **Resolvers** (`services/src/brier_services/resolution/`):
  - BLS API v1, FRED (initial release), and NOAA's Access Data Service.
  - Strict parsers and exact `Fraction` arithmetic.
  - HTTPS to three hosts only, with no redirects; credentials are redacted from evidence.
  - Content-addressed raw evidence and offline recomputation.
  - The `python -m brier_services.resolve` service, which checks each rulebook against the journal before proposing.
- **The launch catalogue** (`markets/`): November CPI and unemployment, the December FOMC decision, the year-end 10-year yield, and Christmas Day in Central Park. Release dates come from the BLS and FOMC calendars; thresholds sit near the latest values.
- **A read model of the journal's pins** (`market_journal`), so the API can show the journaled rules hash and settlement next to the stored rows. It is rebuilt from the journal like every other read model.
- **Docs:** the data-source terms (`docs/data-licenses.md`), the G5 threat-model section, and a dispute runbook (`docs/runbooks/disputes.md`).

## Found during G5, and fixed
1. **The BLS resolver asked for the wrong years** (found by R1, attempt 1).
   - BLS API v1 ignores `startyear` and `endyear` on a GET and answers the latest three years.
   - Unit tests could not see this: they ran on a recorded response that happened to hold the months they needed.
   - What it would have done: a market on a month more than about three years back would have waited for a value that never came, then voided at its deadline instead of settling.
   - The fix: the resolver POSTs the years as JSON, as BLS documents (resolver version 2), and evidence records the request body.
2. **The study's FOMC parser missed 9 of 64 rate decisions** (attempt 2).
   - Causes: the Board's pages write fractions with a non-breaking hyphen ("1‑1/2"), and two statements say "decided today to" and "point, to".
   - Had it gone unnoticed, R1 would have tested fewer decisions than registered while reporting a pass.
   - The study now lists every statement in which it found no decision, and a reviewer can check each one (all five are non-policy statements).
3. **BLS published no October 2025 CPI or unemployment rate** (the 2025 lapse in appropriations; the value is `-`). The resolvers treat it as missing, which voids at the deadline, and the study reports the month as in neither source.
4. **FRED returns values without trailing zeros** ("4" for 4.00, "4.3" for 4.30). Parsing to exact fractions makes this harmless; string comparison would not have been.
5. **Live markets raced the scheduler in the live tests.** The long-window test market's open time had passed, so the scheduler could open it before the test's explicit open. Both live tests now accept either path.

## Measured
- R1, per type (instances = compared, all with 0 mismatches): CPI 12-month change 59, CPI index 59, unemployment 59, FOMC upper limit 64, 10-year yield 249, Central Park maximum 365.
- R2: 855 of 855 decisions identical when recomputed offline.
- Exit test: five markets, two months of schedule, about 6.5 s.
- Journal v3 checks: a 10-million-command differential run (0 divergences), a 100-kill crash run (0 failures), and 33 million fuzz inputs on the journal decoder and wire parser.

## Deviations
- **Exit-test responses.** The launch markets' values do not exist yet, so their responses are recorded ones with one value added (see the exit criteria). The first real resolutions happen when each value is released: 4, 9, and 10 December 2026, around 27 December, and 4 January 2027.
- **R1's definition of an instance.** The registration did not say whether a date neither source reports counts. The study counts periods at least one source reports; others are listed. The first results were seen before this was written down, so this is stated as an interpretation, not as registered.
- **Shorter journal-format runs.** After the v3 change, the crash study ran 100 kills (the registered study is 1,000) and the fuzzers 1 minute each, not 1 hour. The record layout is unchanged, and so are the checksum and recovery code; only the payload encoding grew.
- **The test clock.** A file-driven clock is compiled in only with the `test-clock` feature, and only into `target/test-clock`. A normal build cannot move its clock.

## Compared with Kalshi and Polymarket
- **Ahead:**
  - Every market's rules are pinned by hash in the journal, and the settlement names its evidence by hash.
  - Anyone can re-derive an outcome from the stored source response.
  - Kalshi publishes rulebooks but does not commit them cryptographically. Polymarket's UMA disputes are open to all, but they rest on token-holder votes, not on a recomputable source response.
- **Behind:**
  - The catalogue has five markets with one source each. Kalshi lists thousands, with multiple sources and human market operators.
  - Two colluding admins can still settle wrongly if nobody disputes within 24 h.
  - Admins have no second factor until G6.
  - There is no alert when the stored rows and the journal disagree; only the API shows it.

## Exam questions
1. The engine carries the rules and evidence hashes into its events but stores neither. What would storing them in engine state have cost, in snapshots, state hashes, and the differential tests? And what does the `market_journal` read model give instead?
2. The settler marks a resolution `settling` before it sends `settle`. Walk through a crash at each point (before the claim, after the claim but before `settle` is durable, after `settle` is durable but before the row update). Show that the market settles exactly once, with the right evidence hash.
3. BLS's GET ignored its year parameters, and every unit test passed. Name the property of the test data that hid it, and design one test that would have caught it without network access.
4. A series that can be watched live must close before its period starts; a released-after-period series must have its release after the period ends. Give a market that the first rule protects and the second does not, and say what goes wrong for traders without the rule.
