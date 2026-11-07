# G6 plan: web app

Reads: SPEC §0 item 8, §6, §7 (forecaster scores), §13, §16. Exit: Playwright flows and axe checks pass.

## The problem
Everything so far is reachable only through an API. G6 is the product people see. It has to feel as quick and clear as Kalshi or Polymarket, and show what they do not: each market's pinned rules and settlement evidence, and a scored record of how good each user's forecasts are.

## Scope
- **Pages:**
  - Market list (state, last price, volume, close time, category).
  - Market page: order-book ladder, price chart, trade ticket, recent trades, rules in words with the rulebook and its hash, and the resolution timeline with evidence.
  - Portfolio: positions, open orders, fills, P&L.
  - Forecast panel on every open market.
  - Scorecard and two leaderboards (skill; settled trading profit).
  - Sign-in with TOTP.
  - Admin console: list markets from rulebooks, confirm and withdraw resolutions, rule on disputes, kill switches, audit log.
- **Not in G6:** the surveillance case view (G8, when cases exist), and mobile apps (out of scope, SPEC §1.3). The site is responsive.

## Backend added in G6 (Rust gateway, PostgreSQL)
1. **Forecasts** (SPEC §7).
   - `PUT /v1/markets/{id}/forecast` with a probability, 1 to 99 percent. Allowed only while the market is open and before its close time; frozen after.
   - Timestamped by the server, append-only: a change adds a row and never edits one, so nothing can be backdated.
   - `GET /v1/forecasts` returns the caller's own forecasts.
2. **Scoring.** Skill is averaged over the market's open period, but journal commands carry no timestamps (protocol §8), so the exact price path in time is not recorded. Two options:
   - Add a sequencer timestamp to every command, which is journal format v4 and touches every record.
   - **Chosen:** a scorer task samples, once a minute, each open market's price and every user's standing forecast into `score_samples`.
   - **Market price:** the last trade; before the first trade, the book midpoint; with one side only, that side; with an empty book, 50.
   - **Brier, in units of 1/10,000:** (p − 100·outcome)², with p in percent.
   - **Skill per sample:** market score minus user score, and 0 before the user's first forecast.
   - **Average:** over the samples, rounded half away from zero.
   - Void markets are not scored. With minute samples, a market open for N minutes is scored on about N points, so a forecast's weight error is at most one minute.
3. **Leaderboards.**
   - Skill: average skill over at least `MIN_SCORED_MARKETS` = 10 scored markets.
   - Profit: settled trading P&L. Per market it is the signed trade cash (−p·q bought, +p·q sold) plus the final position × 100 for YES, × 0 for NO, or × 50 for void. It is computed from the fills read model and the journaled outcomes.
4. **Market metadata for the list.** `GET /v1/markets` gains the rulebook title, category, close time, last price, and 24 h volume.
5. **TOTP** (RFC 6238; SHA-1, 6 digits, 30 s, the parameters every authenticator app supports). Setup returns a secret and an `otpauth://` URI; enabling needs one valid code. Login asks for a code once TOTP is on, and admin actions need a session that used TOTP. Secrets are stored encrypted with a key from the environment.

## Frontend
- **Next.js (App Router) and TypeScript, strict.** It talks only to the public REST and WebSocket APIs, through the same rules as any client: the book via snapshot plus deltas with gap recovery, and orders with client order IDs.
- **No component framework.** Plain CSS with design tokens, and light and dark themes.
- **Accessibility (WCAG 2.2 AA):**
  - Every flow works by keyboard.
  - The ladder and the chart have text alternatives.
  - Colour is never the only signal (YES and NO also carry text and shape).
  - Focus is always visible.
- **Testing:** Playwright end-to-end flows against a real gateway (the test-clock build from G5) and axe-core on every page. The flows:
  - register, enable TOTP, sign in;
  - trade, amend, and cancel;
  - forecast;
  - the market resolves; the scorecard updates;
  - admin: list a market, confirm a resolution, rule on a dispute.

## Dependencies (to be approved before installing)
- **npm:** `next`, `react`, `react-dom`, `typescript`, `@types/*`, `@playwright/test` (and its Chromium download), `@axe-core/playwright`, `eslint` and its Next config. Each is pinned to a version at least 14 days old, with an `npm audit` gate in CI.
- **Rust:** `hmac` and `sha1` (RustCrypto, the same family as `sha2`) for TOTP.

## Units
1. This plan. Forecasts: table, endpoints, freezing at close, and tests.
2. Scorer: samples, scores at settlement, both leaderboards, endpoints, and tests (including a hand-computed scoring example).
3. Market metadata on `GET /v1/markets`; TOTP (after the Rust dependencies are approved).
4. Web scaffold, design tokens, layout, the market list, and the market page with ladder, chart, and ticket (after the npm dependencies are approved).
5. Portfolio, forecast panel, scorecard, leaderboards, and the rules and evidence pages.
6. Admin console.
7. Playwright flows and axe checks in CI, the G6 threat-model section, and the gate review.
