# Launch catalogue

One rulebook per market (format: `docs/rulebook.md`). An admin lists a market by sending its rulebook to `POST /v1/admin/markets`; the gateway validates it and records its SHA-256 in the journal, so a file here can never change after listing.

| ID | Market | Source | Trading closes | Value published |
|---|---|---|---|---|
| 101 | CPI inflation above 3.5% in November 2026 | BLS `CUUR0000SA0` | 10 Dec 2026 13:00 UTC | 10 Dec 2026 13:30 UTC |
| 102 | Unemployment rate at 4.2% or higher in November 2026 | BLS `LNS14000000` | 4 Dec 2026 13:00 UTC | 4 Dec 2026 13:30 UTC |
| 103 | Fed cuts in December 2026: target upper limit below 4.00% | FRED `DFEDTARU` | 9 Dec 2026 18:55 UTC | 9 Dec 2026 19:00 UTC (statement) |
| 104 | 10-year Treasury yield above 4.75% on 31 December 2026 | FRED `DGS10` | 31 Dec 2026 00:00 UTC | 4 Jan 2027 21:00 UTC |
| 105 | Central Park reaches 40°F or warmer on Christmas Day 2026 | NOAA GHCN-Daily `USW00094728` | 25 Dec 2026 00:00 UTC | 27 Dec 2026 00:00 UTC |

Release dates are from the BLS release calendars and the Federal Reserve's FOMC calendar, read 7 November 2026. Thresholds sit near the latest values (CPI 3.7% for September, unemployment 4.1% for October, upper limit 4.00%, 10-year yield 4.84% on 5 November), so each market starts near even odds. `services/tests/test_launch_catalogue.py` resolves all five end to end.

There are no sports markets, by design (SPEC §1.3).
