# G10 plan: public launch

Reads: SPEC §0, §1, §7, §16; `docs/threat-model.md`; `docs/data-licenses.md`. Rewritten on 8 November 2026. The owner will open Brier to the general public, so G10 changes from "release" (README, report, recorded demo) to a public launch. The G9 live run continues on GitHub Actions and is reviewed as G9 before this gate closes.

## Owner decisions (8 November 2026)
- **Hosting:** an Oracle Cloud Always Free VM. The owner creates the account; Oracle asks for a card to verify identity and does not charge it. This settles O5.
- **Sports:** in scope. The owner registers free keys for balldontlie (NBA, NFL and other US leagues) and football-data.org (12 soccer competitions). "No sports contracts" was a scope choice in this spec, not an owner rule, and it is withdrawn.
- **Minimum age:** 18. Users confirm their age at sign-up.

## What a public user needs that Brier lacks
| Gap | Why it matters in public | Unit |
|---|---|---|
| Terms, privacy notice, house rules; 18+ confirmation | Strangers need to know the rules and that credits are worthless; an age line for a betting-style product | U1 |
| Account recovery | Users forget passwords. There is no email, so recovery codes are issued at sign-up (shown once, stored hashed) | U1 |
| Account deletion and data export | Expected of any public site; required for users under GDPR and CCPA | U1 |
| Sign-up abuse limits | One person must not farm thousands of accounts for credits or leaderboard spots | U1 |
| A way back after going broke | One bad bet would otherwise end the game for good. A daily allowance, equal for everyone, logged as a grant | U2 |
| Public profiles | Seeing who is good, and following them, is half of why people return (Polymarket, Manifold) | U3 |
| Activity on each market | Recent trades and top holders on the event page | U3 |
| Notifications | Order filled, market settled, dispute ruled: an in-app bell | U3 |
| Sharing and search engines | Link previews (Open Graph images), sitemap, robots, installable app (manifest, icons) | U4 |
| Breadth | 35 events is thin. More weather questions and cities, more economic series, sports | U5, U6 |
| Running 24/7 | Hosting, TLS, backups, restarts, monitoring, runbooks | U7 |

## Units
**U1. Public accounts.**
- Sign-up asks for 18+ confirmation and agreement to the terms; both are recorded with a timestamp.
- Ten one-time recovery codes are issued at sign-up and can be regenerated from the account page. Each is stored as an Argon2 hash. A code resets the password and signs out every session.
- Deleting an account first cancels the user's open orders. Their positions stay in the ledger, because settlement and conservation need them, but every personal field is removed and the username becomes `deleted-N`. Data export returns the user's profile, orders, fills, ledger entries and forecasts as JSON.
- Sign-up is limited per client address (HMAC'd, as for login signals) and per day overall. Limits are named constants with their reasons.
- New pages: `/terms`, `/privacy` and `/rules` (house rules), in plain language. The owner is told these are not legal advice.

**U2. Daily allowance.**
- A user can claim 100 credits once per UTC day. It is a `Grant` ledger entry, so conservation holds and the same rule applies to everyone.
- SPEC §1.2 is amended. The skill leaderboard (Brier score) is unaffected; the profit leaderboard counts profit from trading only, never grants.

**U3. Social layer.**
- `/u/{name}` shows a user's skill score, profit, open positions, recent trades and joined date. The user can make the profile private.
- The event page gets two new tabs: Activity (recent trades) and Top holders.
- Notifications: a read model fed from the journal and a bell in the header. Fills, settlements, disputes and the daily allowance appear there.

**U4. Shareable and installable.**
- Open Graph images per event, rendered with Next's image generation and showing the photo, the question and the top prices.
- `sitemap.xml`, `robots.txt`, a web app manifest and icons.

**U5. More markets from current sources.**
- NWS climate reports already include each day's low, rainfall and snowfall, so these become new daily series for the 12 cities.
- More cities, chosen for climate-report coverage.
- FRED: weekly jobless claims, the daily 10-year yield, weekly mortgage rates. Each source's licence is checked and recorded first.
- Models are backtested under pre-registered hypotheses, as in G9.

**U6. Sports.**
- Resolvers: balldontlie (games and final scores) and football-data.org (matches and final scores).
- Templates: winner of each game (moneyline), total points over or under, and a season-futures event per league where the source supports it.
- Settlement: balldontlie's terms forbid using its data as the sole basis for settling a wager without independent verification. Sports markets therefore use the manual review lane: an admin confirms against the league's official result. The automatic lane is used only where two independent sources agree.
- Pricing model: Elo ratings from past seasons, pre-registered and backtested on held-out games, with a home advantage.
- Licences and attribution are recorded in `docs/data-licenses.md`.

**U7. Deployment.**
- One Docker Compose stack on the VM: gateway, Postgres, resolver, maker, lister, surveillance, web and Caddy.
- Caddy serves TLS from Let's Encrypt, on the owner's domain or a free sslip.io name.
- The journal and a nightly encrypted `pg_dump` go to Oracle Object Storage (free tier); restore is tested.
- Restart on failure, health checks, log rotation and an uptime alert.
- Runbooks: deploy, restore, rotate secrets, halt trading.
- Before go-live, a load test at expected traffic and the threat model are reviewed for public exposure.

## Exit criteria
1. A stranger on a phone can sign up (18+, terms), trade, claim the allowance, recover a lost password with a code, delete the account and export their data. All of this is covered by e2e tests with axe clean.
2. At least 100 open events at a typical moment, across weather, science, economy, rates and sports, all quoted.
3. Every source has a recorded licence decision, and sports settle only with independent verification.
4. The public site runs on the VM with TLS, survives a restart and a restore drill, and the runbooks are followed once end to end.
5. Conservation, determinism and the existing test suites stay green; CI is green on the tagged commit.

## Order
U1, then U2, U3 and U4 (no owner action needed). U5 next. U6 once the keys are in `.env`, built against recorded fixtures until then. U7 once the VM exists.

## Status
Closed on 12 November 2026. See `docs/gates/G10-review.md`.
