# G10 review: public launch

Brier is live at https://132-145-162-222.sslip.io on two Oracle Cloud Always Free servers, at no cost. This review checks the plan's exit criteria against the deployment as it ran on 11 and 12 November 2026.

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| 1. A stranger on a phone can sign up (18+, terms), trade, claim the allowance, recover a lost password with a code, delete the account and export their data, all covered by e2e tests with axe clean | MET | `web/e2e/flows.spec.ts` runs these flows on a Pixel 7 screen and on desktop. One flow signs up, claims the credits, buys an outcome, opens the portfolio and searches. The other claims the credits, resets the password with a recovery code, exports the data and deletes the account. axe (WCAG 2.2 AA) is clean on every page visited. All 11 flows pass. |
| 2. At least 100 open events at a typical moment, across weather, science, economy, rates and sports, all quoted | MET, with one documented exception | 106 open events at 07:00 UTC on 12 November: sports 49, weather 48, science 6, economy 2, rates 1. 105 are quoted. The exception is the Fed decision market, which has had no model since G7: there is no free, licensed futures or OIS data to price it. It can still be traded between users. |
| 3. Every source has a recorded licence decision, and sports settle only with independent verification | MET | `docs/data-licenses.md` covers all eight providers in the registry (BLS, FRED, NOAA, NWS, USGS, SWPC, balldontlie, football-data.org), the Wikimedia photos, and two sources that were turned down (Coinbase; the Wikimedia Analytics API is undecided and unused). Sports markets use manual review: the resolver proposes and an admin confirms against the league's official result. |
| 4. The public site runs on the VM with TLS, survives a restart and a restore drill, and the runbooks are followed once end to end | MET | Caddy serves a Let's Encrypt certificate. Both servers were rebooted: the site was back in 73 s and the exchange in 79 s. A nightly backup was restored into a second gateway, which reached the same sequence number and event count as live. Deploy, backup and restore were each followed as written (`docs/runbooks/deploy.md`, drill record). |
| 5. Conservation, determinism and the existing suites stay green; CI is green on the tagged commit | MET | `check` is green on the tagged commit: Rust, including the engine's conservation and replay tests; Python, with 278 tests; and the browser flows. |

## Problems found during the review, and fixed
- **The market maker ran out of credits.** At about 380 markets, a 25-contract quote on both sides of every market needed more than its sign-up grant. The first refused order also ended the whole pass, so 22 events had no prices. Now quote size shrinks to fit its cash, up to the registered 25. A refused order skips only that side, and the maker claims the daily allowance like any account. It gets nothing other users don't. Recorded in `docs/notes/market-maker.md`.
- **A deploy reported success after its install had failed.** Ubuntu's automatic security update held the package lock. Installs now wait up to 15 minutes for the lock, and a failed install fails the deploy.
- **Phones hid the cash balance, search and Leaderboards,** and the account button had no accessible name. The account menu now carries the balance and Leaderboards, a search button appears on small screens, and the search page has its own box.
- **The amount box accepted whole credits only.** A buy of 1.50 showed a price but was silently blocked by the browser. It now takes cents.

## Differences from the plan
- **Docker Compose was replaced by systemd services on two servers.** Oracle had no Ampere A1 capacity for three days, and the two free E2 micro servers have 1 GB of memory each. Every service still restarts on failure and starts on boot.
- **Backups go to the other server instead of Object Storage.** They are encrypted nightly and kept for 30 days. Object Storage lifecycle rules needed an IAM policy change that was not made.
- **The uptime alert is a GitHub Actions check every 15 minutes.** It fails, and GitHub emails the owner, when the site, the exchange or the events list stops answering.
- **Logs:** journald's default size cap rotates them. No separate log rotation was added.

## Load and exposure
- **Load:** about 14 pages a second at 50 simultaneous users, with no errors (`docs/notes/load-test.md`). The limit is page rendering on the web server's small CPU, not the exchange.
- **Threat model:** reviewed for public exposure (`docs/threat-model.md`, G10 section). The gateway is reachable only on the private network, SSH accepts keys only, and PostgreSQL listens only locally.

## What a visitor gets
- **Markets:** weather highs and lows in 12 cities, earthquakes and geomagnetic storms, CPI, unemployment, the Fed decision, the 10-year Treasury, and every NBA and NFL game, plus five European soccer leagues.
  - Soccer returns around 16 November, when the games after the international break enter the 5-day listing window.
- **Accounts:** sign-up with recovery codes, two-step sign-in, a daily allowance, public profiles, activity and top holders on each market, notifications, data export and account deletion.
- **Pages:** share images, a sitemap, and an installable app.

## Status
G10 is closed. All five criteria are met, with the Fed market's missing quote recorded under criterion 2.
