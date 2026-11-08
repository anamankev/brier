# Threat model

Updated at every gate review (SPEC §13). Each entry gives the threat, its mitigation, the evidence that the mitigation works, and any open gap.

## Components that exist (G0)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Source repository (public) | A secret committed by mistake | gitleaks on every commit (pre-commit) and over the full history in CI; `.env` files ignored | `.pre-commit-config.yaml`, `security.yml` job `secrets` | Hooks can be bypassed locally with `--no-verify`; CI still scans |
| Source repository | Private development files or notes published | Untracked via `.git/info/exclude`; `scripts/private_tooling_check.sh` fails if any are tracked or referenced | `make private-check`, CI job `private-tooling` | None known |
| Dependencies | A known-vulnerable crate or package | Lockfiles committed (`Cargo.lock`, `services/uv.lock`); cargo-audit and pip-audit in CI, weekly as well as per push; Dependabot proposals | `security.yml` | Audits find known advisories only, not malicious new releases |
| Dependencies | A compromised upstream release pulled in silently | Exact versions from lockfiles (`--locked` everywhere); no runtime dependencies in `brier-types` | `Makefile`, `check.yml` | Build-time dependencies (proptest, dev tools) are trusted |
| CI | A workflow given more power than it needs, or a moving action tag swapped | `permissions: contents: read`; third-party actions pinned to commit SHAs | `.github/workflows/*.yml` | None known |
| Domain values | Invalid prices or quantities, or overflowing money arithmetic, reaching later components | Validated newtypes; checked arithmetic; lints forbidding floats, unchecked arithmetic, and panics in library code | `brier-types` tests and proptests; clippy configuration in `Cargo.toml` | None known |

## Components that exist (G1)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Matching engine | A logic bug that mismatches orders, breaks priority, or loses quantity | Rule tests per SPEC §3.1 rule; property tests (never crossed, consistent links, quantity conservation) after every command; differential testing against an independent reference engine | `crates/engine/tests/`, `docs/notes/differential.md` | Both engines share one author and one plan text; a shared misreading is guarded only by the rule tests |
| Matching engine | Nondeterminism making replay diverge from the live run | No clock, randomness, I/O, or unordered iteration in the engine; replay-determinism property test; BLAKE3 state hash | `properties.rs::replay_is_deterministic`, the anti-toy scan in the gate review | The journal (G3) is not built yet; replay currently covers in-memory command lists |
| Matching engine | Out-of-sequence or duplicated commands | Gap-free sequence check; a gap halts the engine | `rules.rs::a_sequence_gap_halts_the_engine` | The sequencer itself arrives in G3/G4 |
| Other users' orders | One account cancelling, amending, or probing another's orders | Ownership checked on cancel and amend; other accounts' orders answer `unknown_order`, so their existence is not revealed | `rules.rs::cancel_only_reaches_the_owners_live_orders` | Authentication of who an account is arrives in G4 |

## Components that exist (G2)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Ledger | A bug that creates or destroys credits | Double-entry entries that must sum to zero (`Entry::new` refuses otherwise); I1 conservation checked after every command in tests and periodically in long runs; the engine halts on violation, never adjusting a balance | `tests/ledger.rs`, `properties.rs`, `docs/notes/differential.md` (G2) | None known |
| Ledger | An account spending credits it has locked, or more than it has | Collateral lock with a worst-case rule proven sufficient (ADR-0005); pre-trade check counting the incoming order as resting; I3 checked for touched accounts after every command | `collateral_must_cover_the_whole_order_exactly`, ADR-0005 proof | The proof is by hand; a mechanized proof is out of scope |
| Play-money boundary | A path that moves credits between users outside matched trades, or that adds credits after signup | Only three entry kinds exist; a property test checks every entry's kind and legs in random streams | `every_entry_is_a_grant_a_trade_or_a_settlement` | Admin re-grants (SPEC §1.2) are not built; they will need the same test |
| Settlement | Paying out more or less than escrow | Escrow = 100 × open interest (I2); settlement must end at exactly zero escrow or the engine halts; void pays 50 to each side (ADR-0002) | `settlement_always_zeroes_escrow`, void and YES rule tests | Who may trigger settlement is G5 (two-step confirmation) |
| Markets | A runaway account or market during an incident | `cancel_all` by account, market, or everything; `halt_all` | Kill-switch rule tests | Who may trigger them, and the audit log, are G4 |

## Components that exist (G3)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Journal | An acknowledged command lost in a crash | Group commit: acknowledgment and events only after the batch's `F_FULLFSYNC` (ADR-0004); recovery of torn tails at most one batch long | 1,000-kill study: 0 of 6,058,014 acknowledged lost | SIGKILL proves ordering, not the SSD's honesty on power loss |
| Journal | Silent corruption of acknowledged data | CRC-32C per record; consecutive sequence numbers; damage outside the last batch stops recovery instead of guessing (ADR-0006) | Corruption and torn-tail property tests | A bit flip in the last 1 MiB of acknowledged data would be read as a torn tail; replication is out of scope |
| Snapshots | A wrong snapshot silently changing state | Restore checks every invariant and the recorded state hash; a damaged snapshot is skipped and the journal replays further | Snapshot property tests; 998 snapshot recoveries in the crash study matched a clean replay | None known |
| Core process | A poison command (one that halts the engine) acknowledged and replayed forever | A halting command drops its whole unsynced batch; nothing in it is acknowledged | `brier-core` exit 3 path | A command that halts the engine will halt it again if resubmitted; it needs operator action (runbook, G9) |

## Components that exist (G4)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| User accounts | Account takeover by password guessing | Argon2id (the `argon2` crate's defaults: 19 MiB, 2 passes, OWASP's minimum), 12 to 256 characters; 30 login or registration attempts per minute per address; an unknown username costs the same hash as a known one | `auth.rs` tests; `rest.rs` login cases | No TOTP yet (G6, with the web login). No breached-password check. No per-account lockout, on purpose: lockout lets anyone lock a victim out |
| Sessions | A stolen or leaked session token | 256-bit random tokens; only their SHA-256 is stored; 24-hour expiry; logout deletes the row; the role is re-read on every request, so a demotion takes effect at once | `rest.rs` logout case | Tokens are bearer credentials: whoever holds one is the user until it expires. No session list or remote sign-out yet |
| Orders | Forged requests | Ed25519 signatures over timestamp, method, path with query, and the body's SHA-256, verified strictly (`verify_strict`); the server stores only public keys; keys are added and revoked only with a session | `rest.rs` signed cases (valid, tampered body, revoked, key endpoints); `signature_verify` fuzz target | The private key's safety is the client's job |
| Orders | Replayed requests | Timestamps more than 5 s from the server clock are refused; a signature seen within the window is refused | `rest.rs` replay and stale cases; `test_client_live.py` replay case | The replay cache lives in the process: right after a restart, a request signed in the previous 5 s could be accepted once. Orders with a `client_order_id` stay idempotent across that. Several gateways would need a shared cache |
| Orders | Duplicate orders from retries | Client order IDs, deduplicated by the engine over each account's last 10,000 accepted orders; a timed-out request says it is safe to retry | Differential tests with 10% reused IDs; `rest.rs` duplicate case | A retry after more than 10,000 newer accepted orders is placed again |
| Gateway | Abuse and denial of service | Token buckets: 200 orders/s per account (burst 400), 500 requests/s per address (burst 1,000); 8 KiB bodies and WebSocket messages; 10 s request timeout; 64 subscriptions and 256 orders in flight per connection; a slow subscriber is resynchronized, never buffered without bound | `limits.rs` tests; `rest.rs` body-limit case; `ws.rs` forced-lag case | Buckets are per process. The per-address limit sees a proxy's address behind a proxy (forwarded headers are not trusted, on purpose). Connections per address are not capped yet |
| Gateway parsers | A crafted request crashing or confusing the server | Unknown fields refused; typed decoding with no clipping; fuzzed | `gateway_request` and `signature_verify` fuzz targets (G4 campaign in `docs/gates/G4-review.md`) | None known |
| Administration | Admin misuse | Admin role checked on every request; a reason (3-500 characters) is required; the audit row is written before the command is submitted and completed with its sequence number and the engine's answer; promotion only through the operator CLI, which also audits | `rest.rs` audit count and role cases | Settlement became two-step in G5 (above). The database role can rewrite `audit_log`; it is not append-only. No admin second factor (G6) |
| Privacy | One user learning about another's activity | Other accounts' orders answer `unknown_order` or 404; order replies show only the caller's fills; public trades and the trades channel carry no account identities | `rest.rs` and `ws.rs` cases | Registration reveals whether a username is taken (rate-limited, as on most exchanges) |
| Read models | History that is wrong or silently stale | Projected from the journal, one transaction per batch with its cursor; every history answer states `as_of` | `rebuild.rs` (live equals rebuild) | None known |
| Dependencies | A fresh, compromised release | Every locked version must have been public for 14 days: Python resolution has a fixed cutoff (`exclude-newer`), and `scripts/dependency_age.py` checks all three lockfiles against crates.io and PyPI whenever one changes. G4's first check found 5 Python dev tools and 16 crates (13 of them also in the fuzzing lockfile) that broke the rule; all were rolled back | `services/pyproject.toml`, `dependency-age.yml` | Cargo has no resolution cutoff, so a `cargo update` can still pick a fresh crate; the check catches it at push time |

## Components that exist (G5)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Rulebooks | Changing a market's rules after people traded on them | The rulebook's SHA-256 (RFC 8785 canonical JSON) travels in `create_market` into the journal; the gateway refuses a second, different rulebook for a market; `GET /v1/markets/{id}/rules` shows the stored rulebook beside the journaled hash and whether they match; the resolver refuses a market whose hashes disagree | `resolution.rs` (listing, rules endpoint); `test_resolution.py` (`rules_mismatch`); 39 shared Rust/Python canonical vectors | Anyone can check, but nothing alerts when the stored row and the journal disagree; the resolver only skips that market |
| Markets | Trading after the answer is known | Every rulebook closes before its release time; a series that can be watched live closes before its period starts; the scheduler closes markets at their close time and catches up after downtime; an admin cannot reopen a market outside its window | `docs/rulebook.md` timing rules (validator tests); `resolution.rs` schedule cases; exit test | A source that publishes early (a leak, an early release) is outside these rules; the market would still be open until its close time |
| Resolution | A wrong outcome from a parsing or arithmetic error | Strict parsers (an unreadable response proposes nothing); exact `Fraction` arithmetic; the registered R1 study: 855 instances against independent comparators, zero mismatches | `docs/notes/resolution.md` §4; `test_resolution.py` malformed cases | R1 compares current vintages; the first-release policy is tested only by construction and the exit test. The agencies' own errors pass through |
| Resolution | A spoofed or tampered source response | HTTPS only, to three named hosts, no redirects, 4 MiB cap; the evidence names the raw body by SHA-256 and the settle command carries the record's hash; anyone with the stored body can decide again offline (R2) | `fetch.py` checks and tests; R2 (855 decisions recomputed identically with networking disabled) | TLS trusts the system's certificate store. The raw body is on the resolver's disk (`var/evidence`), not replicated; losing it loses the ability to re-check, not the settlement |
| Resolution | One person settling a market wrongly, alone or with a compromised resolver key | A resolver can only propose; confirming needs an admin who is not the proposer (database constraint and handler check); a 24 h dispute window open to every position holder; an open dispute stops settlement; rulings are by an admin who is not the proposer; every step audited | `resolution.rs` (four eyes, rulings, disputes); `0004_resolution.sql` CHECK constraints | Two colluding admins (one proposes, one confirms) can settle wrongly if no holder disputes in 24 h. The ruling admin may be the confirmer. Admin accounts have no second factor yet (G6) |
| Resolution | Races: a dispute, a withdrawal, and the settler acting at once | Each step is one conditional SQL statement; the settler claims (`settling`) before sending `settle`, after which nothing can withdraw or dispute; a crash after `settle` is finished from the journal only if the journaled outcome and evidence hash agree | `resolution.rs` crash case; scheduler design in `docs/resolution.md` | The dispute and the claim rely on one clock; a clock stepped backwards could let both succeed |
| Resolution | Dispute spam | Position holders only, one dispute per holder per resolution, the per-account limit, and a 3-500 character reason | `resolution.rs` dispute cases | A holder with many accounts can file many disputes (multi-accounting is G8) |
| Gateway clock | Moving the clock to open, close, or settle early | The schedule, proposals, and windows read the system clock; a file-driven clock exists only in builds with the `test-clock` feature, built into its own target directory by the exit test | `clock.rs`; `test_launch_catalogue.py` | A wrong system clock on the host moves everything; there is no external time check |
| Credentials | Leaking the FRED API key | Read only from the environment (`.env`, git-ignored); replaced by `REDACTED` in every evidence URL and study manifest; never logged | `test_resolution.py` redaction cases; study manifest | The key is in the process environment of the resolver and the study |

## Components that exist (G6)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Accounts | Account takeover with a stolen password | TOTP second factor (RFC 6238) for anyone who turns it on; each code accepted once; admin actions need a sign-in that used a code; secrets encrypted under `BRIER_TOTP_KEY` | `totp.rs` RFC vectors; `rest.rs` admin-without-TOTP case; browser flow 1 | TOTP is optional for traders. No recovery codes yet: losing the authenticator means an operator must reset the account |
| Sessions in the browser | Token theft by injected script | The token lives in an httpOnly, SameSite=Strict cookie that page scripts cannot read; the browser talks to `/api/v1/*`, which adds the token server-side | `web/app/api/v1/[...path]/route.ts`; browser flows | Server-side rendering trusts the cookie as-is; a stolen cookie works until expiry like any session |
| Mutating requests | Cross-site request forgery | SameSite=Strict cookie, and every non-GET call through the web server must carry `x-brier-request: 1`, which a cross-site form or image cannot send | proxy route | No per-session CSRF token; the header and SameSite are the defence |
| The web app | Injected script or framing | Content Security Policy (`default-src 'self'`, sockets only to the gateway, `frame-ancestors 'none'`), `X-Frame-Options: DENY`, `nosniff`; React escapes all text; evidence is shown as text, never as HTML | `next.config.ts` | Scripts allow `'unsafe-inline'` because Next.js inlines its bootstrap; per-request nonces would remove it |
| Market data in the browser | A book that silently misses an update | Snapshot then deltas, each with a sequence number; any gap resubscribes for a fresh snapshot (the same rule as the Python reference client) | `web/lib/book.ts`; Python tracker tests | The browser book has no automated gap-injection test of its own; the rule is shared with the tested Python client |
| Leaderboards | Privacy of usernames and results | Leaderboards show usernames, scores, and settled profit only; no balances, orders, or positions of others | `forecasts.rs` | There is no opt-out from the public leaderboards yet |
| Dependencies | A compromised npm package | npm resolves nothing published after a fixed date (`before` in `web/.npmrc`), every npm package is in the 14-day check, and CI audits the runtime dependencies | `scripts/dependency_age.py` (393 npm packages); `check.yml` web job | One moderate advisory in dev-only lint tooling (`minimatch`) is accepted until its fix is 14 days old (10 November 2026) |

## Components that exist (G7)

| Asset | Threat | Mitigation | Evidence | Gap |
|---|---|---|---|---|
| Market-maker account | A stolen maker key trading recklessly | The maker is an ordinary user: signed requests, the per-account order rate limit, collateral checks, self-trade prevention; no admin role and no special limits; play money | `service.py`; gateway auth tests | The position limit lives in the maker's own code; the exchange has no per-account position cap, so a stolen key is limited only by the account's credits |
| Fair value | A wrong or poisoned input mispricing quotes | Inputs only from BLS, FRED, and NOAA over HTTPS to the resolvers' allowlist, no redirects; probabilities clamped to 2-98%; quotes pulled when inputs fail or are over 24 hours old; inventory skew and the position limit cap the loss from one wrong price | `test_marketmaker_service.py` stale case; `test_fairvalue.py` | A source that returns a plausible but wrong number is quoted on until it is corrected; there is no cross-check between sources |
| Markets with no model | Quoting a price the maker cannot defend | The FOMC market is not quoted at all (no free, licensed futures or OIS data) | `test_marketmaker_service.py` | That market's liquidity depends on traders alone |
| Simulated activity | Simulated orders mixing with real users' markets | M1-M3 run on in-process copies of the reference engine; M4 and the demo run a separate test-clock gateway with its own database schema and journal | `sim/market.py`; `live_study.py` | Nothing technical stops someone from pointing the service at a real exchange with a test account; that is a policy (SPEC §10) |

## Components to come (from SPEC §13), owned by their gates

| Gate | Threat | Planned mitigation |
|---|---|---|
| G1-G2 | Engine or ledger bug that creates or destroys credits | Invariants I1-I5 checked after every event; differential tests against the reference engine; halt on violation |
| G3 | Acknowledged order lost in a crash; corrupt journal | Sync before acknowledgment (F_FULLFSYNC on macOS); CRC32C per record; SIGKILL crash harness |
| G8 | Multi-accounting and collusion | Account-linkage signals in surveillance; credits have no value, which limits the incentive |
