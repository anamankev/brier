# Resolution: do the resolvers read the sources correctly? (G5)

Status: COMPLETE (registered before any resolver code existed; the registering commit is the first commit that contains this file). Results in §4.

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
Run with `make resolution-study` on 7 November 2026 (responses retrieved 23:17-23:19 UTC; hashes in `var/study/results.json`). Three attempts; the windows, comparators, and equality rules are as registered in all three.

| Attempt | Code | Outcome |
|---|---|---|
| 1 | G5 unit 6 (`c5595a0`) | **BLS types untested** (20-32 of 59 months on the resolver side). Cause: the BLS resolver asked with a GET, and BLS API v1 ignores `startyear` and `endyear` on a GET, answering the latest three years only. A market on a month older than that would have waited, then voided wrongly. **Fixed:** the resolver POSTs the years as JSON (resolver version 2). The other four types passed. |
| 2 | BLS fix | All BLS types passed. The FOMC comparator matched only 55 of 64 rate decisions: the Board's pages write fractions with a non-breaking hyphen ("1‑1/2"), and two statements say "decided today to" and "point, to". **Fixed in the comparator parser** (study code only; no resolver change). |
| 3 | both fixes | **All pass**, below. |

**R1 (values): passed for every type.**

| Type | Instances | Compared | Mismatches | One-sided | In the window, in neither source |
|---|---|---|---|---|---|
| CPI 12-month change | 59 | 59 | 0 | 0 | 2025-10 (not published: lapse in appropriations) |
| CPI index level (3 decimals) | 59 | 59 | 0 | 0 | 2025-10 |
| Unemployment rate | 59 | 59 | 0 | 0 | 2025-10 |
| FOMC upper limit | 64 | 64 | 0 | 0 | none |
| 10-year Treasury yield | 249 | 249 | 0 | 0 | 116 weekend and holiday dates |
| Central Park maximum (°C) | 365 | 365 | 0 | 0 | none |

- An instance is a period that at least one source reports a value for. The registration did not define it; this reading is the one under which "a date present in only one of the two sources" is meaningful. Under it no type is near the 2% missing limit.
- FOMC: 69 statements were fetched (January 2019 to October 2026). The comparator found a target-range decision in 64. The other five state none: 11 October 2019 (reserve management purchases), 23 and 31 March 2020 (emergency facilities), 27 August 2020 and 22 August 2025 (longer-run goals and strategy).

**R2 (evidence determines the decision): passed for every type.** Every decision against the median threshold (CPI 3.4%, index 312.332, unemployment 4.0%, upper limit 3.75%, yield 4.29%, 64 °F) was the same when computed from the response as it arrived and when recomputed from the stored bytes alone, in a separate process with networking disabled: 59, 59, 59, 64, 249, and 365 decisions, no differences.

**What this does not show:** the agencies' own figures being right (both sides derive from them), and the "first release counts" policy, which current-vintage comparisons cannot test (covered by construction and by the exit test).
