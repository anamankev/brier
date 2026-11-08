# Rulebook format (schema 1)

Every market is listed from a rulebook: a TOML file in `markets/`, sent to the gateway as JSON. The rulebook says, mechanically, how the market resolves. The rules hash is the SHA-256 of its canonical JSON (RFC 8785). It is written into the market's `CreateMarket` command, so the journal pins the exact rules a market was listed under.

Both validators enforce this format: `crates/rules` (Rust, used by the gateway) and `brier_services.rules` (Python, used by the resolvers). The shared files `crates/rules/tests/vectors.json` and `crates/rules/registry.json` hold the two to the same answers.

## Example
```toml
schema = 1
market = 101
title = "CPI inflation above 3.0% in September 2026"
question = "Will the 12-month change in CPI-U for September 2026 be above 3.0%?"
observation = "2026-09"
revision = "first_release"
void = ["no_value_by_deadline", "source_discontinued"]

[source]
provider = "bls"
series = "CUUR0000SA0"
url = "https://data.bls.gov/timeseries/CUUR0000SA0"

[condition]
transform = "pct_change_12m"
op = ">"
threshold = "3.0"

[schedule]
open = "2026-09-01T13:30:00Z"
close = "2026-10-15T12:00:00Z"
release = "2026-10-15T12:30:00Z"
deadline = "2026-12-15T12:30:00Z"
```

The rule text people see is generated from these fields, so it cannot disagree with what the resolver checks:

> Resolves YES if the 12-month percent change in CPI-U, U.S. city average, all items, not seasonally adjusted, rounded to 1 decimal place, for the month 2026-09, as first published by the source, is above 3.0 percent. Resolves NO if it is published and is not. Voided if no value is published by 2026-12-15T12:30:00Z, or if the source discontinues or redefines the series before publishing it.

## Fields
Every field is required, and unknown fields are refused, so a rulebook has exactly one JSON form.

| Field | Meaning | Rule (error code) |
|---|---|---|
| `schema` | Format version | Must be 1 (`unsupported_schema`) |
| `market` | Market ID | Not 0 (`market_id`) |
| `title`, `question` | What traders read | Non-empty; at most 120 and 500 characters; no surrounding spaces; no control characters (`title`, `question`) |
| `source.provider`, `source.series` | Where the value comes from | Must be in the registry, `crates/rules/registry.json` (`unknown_series`) |
| `source.url` | The series' public page | Must equal the registry's (`source_url`) |
| `observation` | Period measured | `YYYY-MM` or `YYYY-MM-DD`, whichever the series uses (`observation`) |
| `condition.transform` | `level` or `pct_change_12m` | Allowed for the series (`transform`) |
| `condition.op` | `>`, `>=`, `<`, `<=` | |
| `condition.threshold` | Decimal string | Canonical: no leading zeros, `+`, trailing `.`, or `-0`; at most the series' precision in decimal places (`threshold`) |
| `schedule.*` | `open`, `close`, `release`, `deadline` | Exactly `YYYY-MM-DDTHH:MM:SSZ`, years 2000-2100 (`time`); open < close < release < deadline (`schedule_order`); deadline at most 92 days after release (`deadline_too_far`) |
| `revision` | `first_release` | The only policy: the first value fetched after the release time counts |
| `void` | Void conditions | Sorted, no repeats, and must include `no_value_by_deadline` (`void_conditions`) |

**Two timing rules prevent trading on a known answer:**
- **Every market closes before its release time.** Nobody can trade after the number is out.
- **A series that can be watched while its period runs closes before the period starts** (`close_after_observable`). This covers market yields and temperatures. Otherwise traders could watch the answer form live.

A series published only after its period ends must also have its release time after that end (`release_before_period_end`). This catches a misdated release.

## Registry
| Provider | Series | Period | Transforms (precision) | Watchable live |
|---|---|---|---|---|
| BLS | `CUUR0000SA0` CPI-U, not seasonally adjusted | month | `pct_change_12m` (1), `level` (3) | no |
| BLS | `LNS14000000` unemployment rate | month | `level` (1) | no |
| FRED | `DFEDTARU` fed funds target, upper limit | day | `level` (2) | no; announced before the effective day |
| FRED | `DGS10` 10-year Treasury yield | day | `level` (2) | yes |
| NOAA | `USW00094728/TMAX` Central Park daily maximum | day | `level` (0), °F | yes |
| NWS | `CLI/{NYC, ORD, LAX, MIA, BOS, SEA, DEN, ATL, PHX, DCA, AUS, DFW}` official daily maximum from each city's daily climate report | day (local standard time) | `level` (0), °F | yes |

Adding a series means reading its terms (`docs/data-licenses.md`), writing its resolver, and adding it to both registries and `registry.json`. Tests fail if the three disagree.

## Schema 2 (G9, in progress)
Schema 1 rulebooks stay valid and keep their hashes. Schema 2 adds three things, and each is covered by the rules hash.

### Events
An `[event]` table groups markets the way Polymarket's events do:

```toml
[event]
slug = "nyc-high-2026-11-12"
title = "Highest temperature in New York on November 12?"
category = "weather"
tags = ["new-york", "daily"]
series = "nyc-daily-high"   # a recurring series; date tabs link its events
exclusive = true            # exactly one market in the event resolves YES
outcome = "54-55°F"         # this market's label inside the event
```

- Every market of an event has the same `event` table apart from `outcome`.
- For an exclusive event, the conditions must partition the line: the gateway refuses a listing whose ranges overlap or leave a gap with the event's other markets.

### Ranges
`condition.op = "between"` takes `threshold` and `upper`, both inclusive. It is used for the inner brackets; the outer ones keep `<=` and `>=`. A temperature event is then `<= 51`, `52-53`, ..., `>= 60`.

### Review
- `review = "automatic"` applies only to series the registry marks `machine_checkable`.
- It lets the scheduler confirm a resolution when an independent re-fetch, at least 10 minutes after the proposal, computes the same outcome.
- The dispute window is 1 hour for markets that trade a week or less, and 24 hours for longer ones.
- `review = "manual"` (the schema 1 behaviour, and the default) keeps the four-eyes confirmation.
