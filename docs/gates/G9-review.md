# G9 review: a live catalogue

## Exit criteria
The 72-hour live run ran on GitHub Actions as 15 chained parts (run 42482025897), from 8 November 2026 12:51 UTC to 11 November 2026 12:52 UTC, against a real gateway on the real clock and real public sources. The results come from the final part's artifact (`live-g9-results-15`: `summary.json` and the full event log).

| Criterion | Status | Evidence |
|---|---|---|
| At least 40 markets open at every moment | MET | At least 93 open in every one of 859 five-minute samples; no gap over 10 minutes in 71.9 hours observed |
| They span at least 5 categories | NOT MET | Two categories were open throughout: weather and science. See below |
| At least 20 settle within 72 hours of listing (corrected before the run) | MET | 97 markets settled, all within 72 hours of their event's first listing: median 50.6 hours, range 38.6 to 62.6 |
| Every settled market has an evidence record, and the recomputed outcome matches | MET | All 97 went through the automatic lane: a proposal with its evidence, then a second fetch at least 10 minutes later that agreed (97 `resolver_verified`, no `verify_mismatch`) |
| The maker quotes at least 90% of open markets for at least 95% of the run | MET | 97.8% of samples had at least 90% of open markets quoted |
| Playwright flows and axe on every new page | MET | The event flow (browse a category, open an event, buy by amount, see it in the portfolio, settle) passes; axe is clean on all six new pages |

## The category shortfall
- **The plan's correction expected four categories and said a fifth was not possible.** The run found two.
- **Why two, not four:** the live run lists only the recurring catalogue (city highs, earthquakes, geomagnetic storms). Economy and rates exist as the launch catalogue's monthly and year-end markets, which the live run did not list, so they were never open in it. The run reported this as it stood rather than adding markets to reach the number.
- **Where it is fixed:** sports are now in scope (owner, 8 November 2026). G10's exit criterion 2 requires at least 100 open events across weather, science, economy, rates and sports at a typical moment, measured on the public deployment.

## What was built
- **Events and recurring series:** rulebook schema 2 with events and outcome labels; the events API; and a lister that lists every series two days ahead from real forecasts.
- **Automatic review:** a second fetch after at least 10 minutes confirms a machine-checkable result; a mismatch is held for an admin.
- **Resolvers:** NWS daily climate reports (12 cities), USGS earthquake counts, and NOAA SWPC geomagnetic activity, each tested on recorded responses.
- **Backtested models** (`docs/notes/catalog-models.md`): temperature beat climatology in 12 of 12 cities and was calibrated; the earthquake hypotheses were not supported, and an exploratory empirical model is used instead.
- **A Polymarket-grade interface:** home, categories, event pages with multi-outcome charts, a trade ticket with market and limit orders, movers, search and a watchlist.

## Status
G9 is closed with one criterion not met (categories), recorded above and carried into G10.
