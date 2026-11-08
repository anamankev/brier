# G9 plan: a live catalogue

Reads: SPEC §0 item 8, §7, §8, §15; `docs/data-licenses.md`. Added on 8 November 2026 at the owner's request, between G8 and the release (now G10).

## The problem
Brier has five markets. Polymarket shows thousands, and most of what makes it worth opening is breadth and pace:
- events that group several outcomes;
- recurring series that settle every day or week;
- live prices on every card;
- the day's movers;
- a market that always takes the other side.

To be worth playing, Brier needs dozens of live markets at any moment, many settling within a day, all quoted, and a browsing and trading interface at the level of Polymarket's. It must do this without breaking its own rules:
- play money only;
- no sports contracts;
- every market settled from a free public source whose terms allow it, with evidence.

## What Polymarket does, and what Brier takes (studied 8 November 2026)
| Polymarket | Brier |
|---|---|
| **Events** group binary markets: "Fed decision in December?" holds five rate brackets, each with its own Yes and No | The same. An event is a set of binary markets sharing a title, category, and schedule (SPEC §1 already lists multi-outcome events as separate binaries) |
| **Recurring series:** 5-minute and 15-minute crypto, daily temperature by city, weekly, monthly; date tabs on the event page | Series listed automatically by template, with date tabs. No sub-hour series: no permitted price source exists (see Sources) |
| **Category pages:** a sidebar of subcategories with counts, a grid of event cards (top two outcomes with Yes and No, volume, recurrence badge, a gauge for single-question events) | The same layout, in Brier's own visual identity |
| **Home:** a featured event with a multi-line chart; Breaking (biggest 24-hour moves) and Hot topics alongside; topic chips over all markets | The same sections, from Brier's own read models |
| **Event page:** multi-outcome chart with 1H to ALL ranges; outcome rows with Buy Yes and Buy No prices and volume; trade ticket (Buy or Sell, Market or Limit, amount with quick-adds); order book; rules, resolution source, resolver; holders and activity | All of it, plus what Polymarket does not show: the rulebook hash, the evidence record, and the settlement audit trail |
| **Comments, combos (parlays), AI "market context", app promotions, sports and esports** | Not taken. Comments need moderation that Brier cannot staff; parlays and sports are out of scope (SPEC §1); generated summaries are not evidence |

## Sources: which real-world series can settle markets
The rule (`docs/data-licenses.md`): free, public, authoritative, and terms that allow fetching values to settle play-money markets and showing them to users.

| Category | Series | Source | Status |
|---|---|---|---|
| Weather | Daily high temperature in about 12 US cities; tomorrow's brackets | NWS observations (api.weather.gov, U.S. government work, no copyright); GHCN-Daily as the slower authoritative check | Usable |
| Weather (global) | Daily highs outside the US | Open-Meteo, CC BY 4.0, free tier non-commercial (Brier is non-commercial play money), attribution required | Usable with attribution |
| Science | Earthquakes of magnitude 6+ this week; magnitude 7+ this month | USGS earthquake catalog (public domain) | Usable |
| Science | Geomagnetic storm (Kp of 5 or more) today | NOAA Space Weather Prediction Center (public domain) | Usable |
| Economy | CPI, unemployment, payrolls brackets; initial jobless claims weekly | BLS, Department of Labor (public domain) | Usable (BLS already integrated) |
| Rates | Fed decision brackets per FOMC meeting; 10-year yield daily and weekly brackets | Federal Reserve via FRED public-domain series (`DFEDTARU`, `DGS10`) | Usable (already integrated) |
| Energy | Weekly US regular gasoline price; WTI crude daily | EIA (public domain; free key) | Usable after key |
| Culture and tech | Wikipedia page views of a topic | Wikimedia Analytics API | **Check license first** |
| Crypto | BTC, ETH prices | Coinbase's market-data terms forbid using its data to settle contracts or in an app for other users (read 8 November 2026). No permitted source found yet | **Blocked** until a source's terms allow settlement |
| Indexes, stocks | S&P 500 and others | Licensed by their owners | Not used (O6) |
| Sports, esports | | | Out of scope (SPEC §1) |

## Design
1. **Events in the rulebook.**
   - Each rulebook gains an `event` block: slug, title, category, tags, and `exclusive: true` when exactly one outcome can win. It also gains an `outcome` label, such as "77-78°F".
   - The rules hash covers all of it.
   - A read-model `events` table, and endpoints `GET /v1/events` (filter by category, tag, series, status; sort by volume, 24-hour change, newest, ending soon) and `GET /v1/events/{slug}`.
2. **Series and the lister.**
   - A template per series turns a date into rulebooks: brackets centred on the forecast, schedule, and source.
   - The lister service (role `lister`, signed key) creates the next instances ahead of time through the admin listing endpoint.
   - Every template is unit-tested against the rulebook validator.
3. **Automatic resolution for machine-checkable sources.**
   - Settling a daily market by hand, with a 24-hour dispute window, would leave the catalogue full of finished markets.
   - A rulebook may say `review: automatic`. Its resolution is confirmed by the gateway when an independent second fetch (by the settler, at least 10 minutes after the proposal) computes the same outcome.
   - The dispute window scales with the market's length: 1 hour for a daily market, 24 hours for anything a week or longer.
   - A disagreement, a dispute, or a void condition goes to the admin queue, with four eyes as before.
   - Brier keeps its rule that a market on a value that can be watched as it forms (a temperature, a yield) closes before the period starts. Polymarket trades a city's high during the day itself; Brier does not, because traders watching the thermometer would trade on a known answer.
   - So a daily market lists about two days ahead, closes at the start of its day, and settles the next morning, roughly 30 hours after listing.
4. **Market statistics.** The read models gain per-market 24-hour volume, 24-hour price change, last price, and open interest, for cards, Breaking, and sorting.
5. **Quotes everywhere.**
   - The reference maker gains models for each series: temperature from the NWS forecast with an error spread fitted on past forecasts; earthquakes as a Poisson process on the catalog's rate; geomagnetic storms from the SWPC 3-day forecast.
   - It quotes every open market it has a model for, with the registered parameters.
   - A market with no model shows "no quotes", as the FOMC market does now.
6. **The web app.**
   - Home, category, event, Breaking, and New pages, plus search and a watchlist, per the table above.
   - The trade ticket gains an amount-based market order. It is sent as an immediate-or-cancel limit order at the worst price the amount allows, so the engine is unchanged.
   - Charts use the existing minute price samples.
   - Browser-reviewed at desktop and phone widths, with axe checks.

## Exit criteria
- At any moment of a 72-hour live run on a real gateway (sped up by the test clock where the source allows it):
  - at least 40 markets are open;
  - they span at least 5 categories;
  - at least 20 settle within 48 hours of listing.
- Every market settled in that run has an evidence record, and the recomputed outcome matches.
- The maker quotes at least 90% of open markets for at least 95% of the run.
- Playwright flows: browse a category, open an event, buy an outcome with a market order, see it in the portfolio, and watch it settle. Axe passes on every new page.

## Units
0. Finish G8: the admin case view, the web server's proxy key, and the G8 review.
1. This plan; the data-license entries for NWS, Open-Meteo, USGS, SWPC, DOL, and EIA; and a decision on Wikimedia.
2. Rulebook schema version 2: event, outcome label, automatic review. The validator, canonical form, and both validators (Rust and Python) are tested to agree.
3. Read models and API: events, market statistics, events endpoints; the automatic-resolution lane in the scheduler; OpenAPI.
4. Resolvers for NWS, USGS, SWPC, and DOL, with recorded-response tests, as in G5.
5. Series templates and the lister service.
6. Maker models for the new series, backtested on past data before use (closing the G7 backtest gap for these series).
7. Web: home, category, and event pages; the trade ticket with market orders; Breaking, New, search, and watchlist.
8. The 72-hour live run, Playwright flows, and the G9 review.
