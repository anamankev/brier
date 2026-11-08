# G6 review: web app

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| Playwright flows and axe checks pass | MET | `web/e2e/flows.spec.ts`, 6 flows in Chromium against a real exchange (the test-clock gateway on PostgreSQL, seeded with the launch catalogue): sign up and turn on TOTP; the catalogue opened by schedule; buy from the trade panel, fill against another trader, cancel a resting order; record a forecast; confirm in the admin console, dispute as a holder, reject the dispute, settle; the scorecard and leaderboards. axe-core reports no WCAG 2.2 A or AA violation on nine pages: sign-up, account, log-in, market list, market page, portfolio, admin console, scorecard, leaderboards. CI runs it (`browser` job) |

## What was built
- **Forecasts and scores** (SPEC §7). Server-timestamped, append-only forecasts (a database trigger refuses edits); a once-a-minute price sample per open market; Brier skill against the market at settlement in units of 1/10,000; a skill leaderboard (10 scored markets) and a settled-profit leaderboard. Held to a scoring example computed by hand (`crates/gateway/tests/forecasts.rs`).
- **TOTP** (RFC 6238): setup and enable, codes accepted once each, secrets encrypted under `BRIER_TOTP_KEY`, and admin actions only from a sign-in that used a code. The RFC's test vectors pass in Rust and in the Python client.
- **Gateway endpoints for the app:** `/v1/me`, minute price history, richer market list (title, category, close, last price, volume), the admin queue and audit log.
- **The web app** (`web/`, Next.js, TypeScript strict):
  - Design: a dark exchange design system (Barlow Condensed display type, Inter with tabular figures), studied against Kalshi's site but with Brier's own mark and identity. YES and NO always carry their word as well as their colour.
  - Home: a featured market, a live-price strip, market cards (chance, Yes and No prices, 24-hour change, sparkline, volume, a "Rules pinned" badge), upcoming releases, top forecasters, and how settlement works.
  - Market page: the chance and its change, a chart with range tabs and a pointer readout, a two-sided order book and trade tape fed by the WebSocket (snapshot, deltas, resubscribe on any gap), a sticky Buy/Sell panel, the forecast card, the rules with a "pinned in the exchange log" check, a timeline from open to payout, and every resolution with its evidence.
  - Portfolio, scorecard, leaderboards, sign-up and log-in with TOTP, account settings, and the admin console (resolution queue, dispute rulings, listing from a rulebook, market state, kill switches, audit log).
  - Security: the session token in an httpOnly, SameSite=Strict cookie behind a server-side proxy; a required request header on every change (CSRF); a Content Security Policy and framing protection.
- **Supply chain:** npm resolves nothing newer than a fixed date (`web/.npmrc`), the npm lockfile joins the 14-day check (393 packages pass), and CI audits the runtime dependencies.
- **A local demo exchange** (`scripts/demo_stack.py`): the launch catalogue with two simulated market makers and five traders, two hours of trading in fast-forward, so the app can be seen with real exchange output.

## Found during G6, and fixed
1. **Every page failed with a server error when the exchange was unreachable** (found by looking at the app while the gateway was starting). The server-side client now returns an "unavailable" answer instead of throwing; pages show a plain message.
2. **The live feed was blocked by the app's own Content Security Policy** when the gateway's socket was not on the default port: Next.js fixes security headers at build time. The socket address is now read when building too, and the README says so.
3. **The trade panel showed "—" for prices until the WebSocket synced.** It now shows the server's prices until the live book arrives.
4. **Two React rules caught by the linter:** writing a ref during render, and reading the clock during render. Both fixed.
5. **Field names read by screen readers included the decorative unit** ("Contracts contracts"). The units are now hidden from assistive technology; the flows find fields by role and name, so this stays checked.
6. **The local demo's web server shared port 3000 with Docker Desktop**, and stopping it by port killed Docker (see Deviations). The app now runs on port 3005.
7. **The browser flows signed in again in each test and were refused**: each TOTP code works once, as designed. The flows now keep the session like a browser and, when they must sign in again, wait for an unused code.

## Measured
- Browser flows: 6 of 6 pass in about 12 seconds after the build; 9 pages with no WCAG 2.2 A or AA violation reported by axe-core.
- Scoring: the hand-computed example (four minute samples, three forecasters, one too late to count) matches to the unit.
- npm: 393 locked packages, all at least 14 days old; runtime audit clean.

## Deviations
- **Skill is sampled once a minute**, not computed over exact trade times, because journal commands carry no timestamps; a forecast's weight is off by at most a minute (G6 plan, SPEC §7).
- **One accepted advisory:** a moderate denial-of-service advisory in `minimatch`, pulled in only by the lint tooling; its fix is too new for the 14-day rule until 10 November 2026. CI audits runtime dependencies only until then.
- **The surveillance case view** (SPEC §0 item 8) moves to G8, when there are cases to show.
- **`'unsafe-inline'` scripts in the CSP**, because Next.js inlines its bootstrap; per-request nonces are the fix.
- **Docker Desktop stopped three times during the visual review, and the cause was a review command, not the machine.** The web server was being stopped with `lsof -ti tcp:3000 | xargs kill`, which kills every process with any socket on port 3000; Docker Desktop's backend listens there, and its log shows a termination signal at each of those moments. It was first blamed on memory pressure, wrongly. The local app now uses port 3005, and servers are stopped by their own process ID.

## Compared with Kalshi and Polymarket
- **At par:** market list with chance and Yes/No prices, sparklines and daily change, a market page with chart ranges, an order book, and a trade panel; dark, dense, and fast.
- **Ahead:** every market page shows the rules pinned in the exchange log and, once resolved, the evidence record with its hash, matched against the log; a forecaster scorecard that rewards being right without trading.
- **Behind:** no mobile app or native notifications, no comments or social feed, no news, no search beyond titles, and five markets instead of thousands. No onboarding tour.

## Exam questions
1. The browser never sees the session token. Trace a market order from the button to the engine, naming each hop and what each one checks, and say what an attacker who can run script on a Brier page could still do.
2. Forecast skill is averaged over minute samples. Construct a forecaster strategy that would gain from the sampling if samples were taken at predictable times only when the market is quiet, and explain why sampling every minute regardless of activity removes it.
3. The CSP has to name the gateway's socket at build time. What breaks if the gateway moves without a rebuild, how would a user notice, and what design would make the policy a runtime setting?
4. TOTP refuses a code it has accepted once. Explain the attack this stops, and why the browser flows had to be rewritten because of it rather than relaxing the check in test builds.
