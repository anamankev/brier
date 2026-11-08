# The recurring catalogue (G9)

Brier lists its everyday markets itself, from templates, so there is always something open
that settles within two days. Templates: `services/src/brier_services/catalog/templates.py`.
The listing service is `python -m brier_services.catalog run`, run by a user with the
`lister` role.

## The series
| Series | Event | Outcomes | Source | Closes | Settles from |
|---|---|---|---|---|---|
| `{city}-daily-high`, 12 cities | Highest temperature in {city} on {day}? | 7: two open ends and five 2 °F ranges; the forecast high is in the middle range | NWS daily climate report (CLI) | start of the day (UTC) | the first morning-after report, from 15:00 UTC the next day |
| `earthquakes-m5-daily` | How many magnitude 5+ earthquakes on {day}? | 2 or fewer, 3-4, 5-6, 7 or more | USGS ComCat | start of the day | the catalog 48 hours after the day starts |
| `earthquake-m6-daily` | Magnitude 6+ earthquake anywhere on {day}? | one Yes/No market | USGS ComCat | start of the day | the same |
| `geomagnetic-daily` | Geomagnetic storm level on {day}? | No storm, G1, G2, G3 or stronger (NOAA scale) | SWPC planetary K-index | start of the day | all eight 3-hour values, from 03:00 UTC the next day |

The 12 cities are New York (Central Park), Chicago, Los Angeles, Miami, Boston, Seattle, Denver, Atlanta, Phoenix, Washington DC, Austin, and Dallas. Each uses the station of its official NWS climate report.

## Rules every template keeps
- **Days ahead.** Every event lists two days ahead and opens at once.
- **Close before the day starts.** Each value can be watched as it forms, so trading stops before its day begins. Brier does not trade a thermometer someone is watching.
- **Ranges.** Exclusive events cover every possible value exactly once, on the series' precision lattice. The tests check this for 60 days of every series.
- **Automatic review.** Every market is settled by an automatic resolver, confirmed by an agreeing re-fetch, with a 1-hour dispute window (`docs/resolution.md`).
- **Deterministic.** The same day and forecast always give the same rulebooks and hashes. Market IDs are `10,000,000 + day index * 1,000 + series code * 10 + outcome index`, which never collide.
- **No forecast, no event.** A temperature event whose city forecast is unavailable is not listed; the next pass tries again.

## What is not listed, and why
- **Crypto:** no price source whose terms allow settlement (`docs/data-licenses.md`).
- **Stock indexes:** licensed by their owners.
- **Sports:** out of scope.
