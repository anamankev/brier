# Resolution: do the resolvers read the sources correctly? (G5)

Status: REGISTERED. Written before any resolver code exists. The registering commit is the first commit that contains this file.

## 1. Questions
1. For each launch-catalogue market type, does the resolver extract the value the source actually published, for the right period, with exact arithmetic?
2. Is a decision fully determined by its stored evidence, so that anyone holding the raw response gets the same outcome?

## 2. Hypotheses
- **R1 (values).** Over every instance in the fixed windows below, the resolver's value equals the value from an independent comparator: a different system or product than the one the resolver reads, parsed by separate study code that shares no parsing code with the resolver.
  - Refuted by a single mismatch.
  - A date present in only one of the two sources is not a mismatch, but it is counted and reported per market type. If more than 2% of a type's instances are missing from either side, R1 is untested for that type, not passed.
- **R2 (evidence determines the decision).** For every R1 instance, the decision against a fixed test threshold is the same when computed from the live response and when recomputed from the stored raw bytes alone, by a fresh process with no network access. Refuted by a single difference or a missing evidence file.

## 3. Design
The windows are fixed here and not changed after results are seen.

| Market type | Resolver reads | Window | Independent comparator | Equality |
|---|---|---|---|---|
| CPI 12-month change | BLS API v1, `CUUR0000SA0` index; the change is computed by Brier as (index_t / index_{t−12} − 1) × 100, rounded half-up to one decimal, in `Decimal` | 60 reference months, October 2021 to September 2026 | FRED `CPIAUCNS` with `units=pc1` (FRED's own percent-change computation), rounded half-up to one decimal. The index levels are also compared with FRED `CPIAUCNS`, exactly to three decimals | Exact |
| Unemployment rate | BLS API v1, `LNS14000000` | Same 60 months | FRED `UNRATE`, current vintage | Exact, one decimal |
| FOMC upper bound | FRED `DFEDTARU`, on the effective date stated in the statement | Every FOMC statement that states a target range, January 2019 to October 2026 | The statement text on federalreserve.gov ("target range for the federal funds rate at X to Y percent") | Exact |
| 10-year Treasury yield | FRED `DGS10` | Every date in calendar year 2025 | U.S. Treasury daily par yield curve rates (home.treasury.gov), 10-year column | Exact, two decimals |
| Daily max temperature | NCEI Access Data Service, `daily-summaries`, `TMAX`, station `USW00094728`, metric units | Every day in calendar year 2025 | The GHCN-Daily by-station file for `USW00094728` (a different NCEI product, in tenths of a degree Celsius) | Exact, after dividing the comparator by 10 |

- **What R1 tests:** parsing, period alignment, missing-value handling, and arithmetic. It compares sources at their current vintage. It cannot test the "first release counts" policy, because today's responses carry later revisions. That policy is enforced by construction: the evidence is frozen at the first successful fetch after the release time. The G5 exit test covers it by resolving from a recorded first-release response.
- **What R1 cannot detect:** an error in the agency's own figure, since both sides derive from it.
- **R2 thresholds:** each type's median value over its window, rounded to the rulebook's precision. The study prints them.
- **Budget:** BLS v1 allows 25 queries a day; the study needs 2 BLS queries per run (10 years each). Responses fetched by the study are kept under `var/study/` (git-ignored), with SHA-256 hashes and the retrieval time recorded in the results.
- **Command:** `make resolution-study` (added in G5 unit 7).

## 4. Results
Not yet run.
