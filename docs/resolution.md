# Market resolution

How a Brier market goes from listed to settled. Code: `crates/gateway/src/markets.rs`, `resolution.rs`, `scheduler.rs`; tables in `crates/readmodel/migrations/0004_resolution.sql`; endpoints in `docs/api/openapi.yaml`. The study that checks the resolvers' values is `docs/notes/resolution.md`.

## Guarantees
- **The rules cannot change after listing.** The rulebook's SHA-256 travels in `create_market` into the journal. `GET /v1/markets/{id}/rules` serves the stored rulebook with the hash the journal recorded, and says whether they match.
- **Nobody trades once the answer is known.** Every rulebook closes before its release time (and a live-observable series closes before its period starts; `docs/rulebook.md`). The gateway closes the market at its close time, and refuses a manual reopen after it.
- **One person cannot settle a market.** A proposal needs a second admin to confirm it, then a dispute window with no open dispute. There is no settle endpoint.
- **The settlement names its evidence.** `settle` carries the SHA-256 of the evidence record, so the journal proves which evidence decided the outcome.
- **Every step is audited:** who (or `system`), what, why, and the journal sequence number of any command.

## Roles
| Role | Can |
|---|---|
| `trader` | Trade; dispute a resolution in a market where it holds a position |
| `resolver` | Propose outcomes, and nothing else: it cannot trade, confirm, rule, or list |
| `admin` | List markets, change market states, propose, confirm, withdraw, rule on disputes; trade |

Roles are set from the command line: `brier-gateway role <username> <trader|admin|resolver> <reason>` (audited).

## Lifecycle
```
list (admin, rulebook) ──> pre_open ──[open time]──> open ──[close time]──> closed
                                                                               │ after the release time
                          ┌────────────────────────────────────────────────────┘
                          ▼
   proposed ──confirm (another admin)──> confirmed ──[dispute window, no open dispute]──> settling ──> settled
      │                                     │  ▲
      │ withdraw (admin)                    │  │ dispute rejected
      ▼                                     ▼  │
  withdrawn <──────── dispute upheld ─── dispute open
```
1. **List.** `POST /v1/admin/markets` with the rulebook. It must validate (`docs/rulebook.md`), its close time must be in the future, and the market ID must be new. A retry with the identical rulebook is safe.
2. **Schedule.** Once a second, the gateway opens markets at their open time and closes them at their close time, audited as `system`. After downtime it catches up at once; a market past its close time goes straight to `closed`. An admin may halt or close a market at any time, but may open it only inside its trading window.
3. **Propose.** `POST /v1/markets/{id}/resolution`, by a resolver or an admin, for a `closed` market at or after its release time. The body has the outcome, the evidence record, and a reason. A `void` proposal names one of the rulebook's void conditions; `no_value_by_deadline` is allowed only after the deadline. A market has at most one live resolution.
4. **Confirm.** `POST /v1/admin/resolutions/{id}/confirm`, by an admin who is not the proposer. The dispute window starts: 24 hours (`DISPUTE_WINDOW`; `BRIER_DISPUTE_WINDOW_SECONDS` overrides it).
5. **Dispute.** `POST /v1/resolutions/{id}/disputes`, by any account holding a position in the market, once per resolution, while the window is open.
6. **Rule.** `POST /v1/admin/disputes/{id}/rule` with `uphold` or `reject`, by an admin who is not the proposer. Upholding withdraws the resolution; its other open disputes become `moot`, and a new proposal starts again at step 3. Rejecting lets the resolution stand.
7. **Settle.** When the window has ended with no open dispute, the gateway claims the resolution (`settling`) and sends `settle { market, outcome, evidence_hash }`, audited as `system`. A claimed resolution can no longer be withdrawn or disputed.

An admin can withdraw a proposed or confirmed resolution (`POST /v1/admin/resolutions/{id}/withdraw`) until the gateway claims it.

## Evidence record
A JSON object, at most 4 KiB in canonical form (RFC 8785, integers only). The gateway requires and checks four fields; the resolver (G5 unit 5) defines the rest, such as the request URL without credentials, the HTTP status, the SHA-256 of the raw response (kept under `var/evidence/<sha256>`), the parsed value, and the resolver's version.

| Field | Check |
|---|---|
| `market` | equals the market |
| `rules_hash` | equals the market's rules hash |
| `outcome` | equals the proposed outcome |
| `retrieved_at` | `YYYY-MM-DDTHH:MM:SSZ`; no earlier than the release time (the deadline for `no_value_by_deadline`), and at most 5 minutes ahead of the gateway's clock |

The evidence hash is the SHA-256 of the canonical record. The same record can back a later proposal.

## Resolvers (`services/src/brier_services/resolution/`)
One module per source; each builds the request for a rulebook and parses the response strictly. A response the parser cannot read, or an HTTP error, proposes nothing.

| Source | Request | Value |
|---|---|---|
| BLS API v1 (no key) | the observation's year and the year before | The index or rate as published. The 12-month change is (index_t / index_{t−12} − 1) × 100. A month published as `-` (October 2025, during the lapse in appropriations) or not yet published is missing |
| FRED (`FRED_API_KEY`) | the observation date, initial release only (`output_type=4`, real time from that date on) | The value as first released, even when fetched after a revision; `.` is missing |
| NOAA NCEI Access Data Service (no token) | the station and date, metric units | °C converted exactly to °F (× 9/5 + 32); an empty answer is missing |

- **Arithmetic** is exact (`Fraction`). The value is rounded half away from zero to the series' precision, then compared with the threshold.
- **Missing values** wait until the deadline, then propose void (`no_value_by_deadline`). `source_discontinued` is never proposed automatically.
- **Fetching:** HTTPS only, to `api.bls.gov`, `api.stlouisfed.org`, and `www.ncei.noaa.gov`; no redirects; at most 4 MiB; 30 s timeout. Credentials in the URL are replaced by `REDACTED` in the evidence.
- **Evidence:** the record adds to the four checked fields the provider, series, observation, transform, operator, threshold, the rounded value (or why it is missing), the redacted request URL, the HTTP status, the SHA-256 and length of the raw body, and the resolver version. The raw body is stored under `var/evidence/<sha256>`. `recompute` decides again from the record and the stored body alone, offline (R2).
- **Service:** `python -m brier_services.resolve keygen --key-file var/resolver.key` creates the signing key (mode 0600) and prints its public key. A user with the resolver role registers it (`POST /v1/keys`). Then `python -m brier_services.resolve run --gateway URL --key-id N --key-file var/resolver.key` proposes, every 5 minutes, for each closed market without a live resolution. It first checks that the rulebook's hash matches the gateway's rules hash and the journal.

## Races and crashes
- Each step is one conditional SQL statement, so two requests racing on one resolution cannot both succeed; the second gets `conflict`.
- A dispute needs `confirmed` and an open window; the settler claims only after the window and only without an open dispute. Both are judged by the same clock, so a dispute and a claim cannot both succeed.
- **Crash after `settle` is journaled but before the row is marked `settled`:** on its next pass the settler finds the market already settled in the engine. It completes the row from the `market_journal` read model, but only if the journaled outcome and evidence hash equal the resolution's. Otherwise it stops with an error for a person to look at.
- **Crash before `settle` is journaled:** the market is still `closed`, so the settler sends `settle` again.
- **Crash between storing a rulebook and `create_market`:** the scheduler skips the stored rulebook until the market exists; an admin retries the listing with the same rulebook.

## Error codes
`invalid_rulebook` (400; the message starts with the rulebook error code), `forbidden` (403: wrong role, four eyes, or no position to dispute), `not_found` (404), `conflict` (409: the state does not allow it), `too_early` (409: before the open, release, or deadline time). Engine rejections keep their codes (422, `docs/protocol.md`).

## Automatic review (G9)
A schema 2 rulebook on a machine-checkable series may say `review = "automatic"`.
- **Confirmation without a second admin.**
  - A resolver re-fetches the source at least 10 minutes after the proposal's fetch.
  - It sends its own evidence record to `POST /v1/resolutions/{id}/verify`.
  - If the re-fetch computes the same outcome, the gateway confirms the resolution with that record as the confirmation (`verified_by`), in place of a second admin.
- **The dispute window** is 1 hour for a market that traded a week or less, and the usual window otherwise. Disputes work exactly as for manual review.
- **A re-fetch that disagrees is kept** (`verify_mismatch`, shown in the admin queue). From then on only an admin can confirm or withdraw.
- **A void outcome is never confirmed automatically.**
- **Why two fetches:** one fetch can catch a source mid-update or a transient error page that still parses. Two fetches at least 10 minutes apart that agree are the machine version of four eyes. They do not protect against a source that publishes a wrong number and keeps it; the dispute window does.
