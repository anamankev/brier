# G5 plan: markets, rulebook, resolution

Reads: SPEC §7, §2.4, §6, §13, O3. Exit: every launch-catalogue market resolves end to end from a recorded source response.

## The problem
A prediction market is only as good as its resolution. The common complaints about the leading venues are about resolution, not matching: rules worded loosely, outcomes argued over after the fact, and evidence that a trader cannot check. G5 makes every step checkable:
- the rules a market was listed under;
- the data it was settled on;
- who proposed and who confirmed the outcome;
- that nobody could trade once the answer was public.

## Design
```
rulebook file (markets/*.toml, reviewed in git)
   │  admin lists it: gateway validates, canonicalizes (RFC 8785 JSON), hashes (SHA-256)
   ▼
CreateMarket { market, rules_hash } ──> journal        rules stored in Postgres, served at /v1/markets/{id}/rules
   │
   │  scheduler: Open at open_time, Closed at close_time (sequenced commands, audited as "system")
   ▼
resolver (Python, role "resolver") fetches the source after the release time
   │  evidence: request, retrieval time, status, raw response (stored by SHA-256), parsed value, decision
   ▼
proposal ──> an admin confirms (four eyes: never the proposer) ──> dispute window ──> Settle { market, outcome, evidence_hash } ──> journal
                                                   │
                                     a dispute pauses settlement until an admin rules on it
```

### Rules and evidence pinned in the journal
- `CreateMarket` gains `rules_hash` and `Settle` gains `evidence_hash` (32 bytes each), in journal format version 3. Readers still decode versions 1 and 2, which carry no hashes.
- The engine carries both hashes through to its events but does not store them, so snapshots and the state hash are unchanged.
- As a result, the journal alone proves which rules a market was listed under and which evidence settled it. Anyone with the rules file and the evidence record can recompute both hashes.
- The wire format, `docs/protocol.md`, the reference engine, the differential generator, and the fuzz targets change together.

### Rulebook (`docs/rulebook.md`, `crates/rules`, one TOML file per market in `markets/`)
Fields required by SPEC §7:
- question, and the exact YES condition;
- source: provider, series ID, URL;
- observation: the period or date measured;
- release time and timezone;
- revision policy;
- void conditions;
- open, close and resolution-deadline times.

The YES condition is structured, not prose: `value <op> threshold`, so a resolver decides it mechanically.
- `op` is one of `>`, `>=`, `<` or `<=`.
- The threshold is a decimal string.
- The value is computed with exact decimals, never binary floating point.
- A transform is allowed only from a fixed list, for example a 12-month percent change rounded to one decimal place, the way BLS publishes it.

The validator refuses a rulebook if any of these holds:
- a field is missing or unknown;
- a provider or series is not in the source registry;
- **the close time is not before the release time** (otherwise fast traders could trade on a published answer);
- the resolution deadline is before the release time;
- times have no timezone.

Canonical form: RFC 8785 (JSON Canonicalization Scheme), implemented in both Rust and Python. Shared test vectors keep the two byte for byte identical.

### Resolution workflow (gateway and Postgres)
**Roles.** `user`, `admin`, and a new `resolver` role. A resolver can only propose outcomes; it cannot confirm, settle, or trade.

**Tables:**
- `rulebooks` (market, canonical JSON, hash);
- `evidence` (hash, record JSON);
- `resolutions` (market, proposed outcome, evidence hash, proposer, confirmer, state, times);
- `disputes`.

**The flow:**
1. **Propose.** A resolver, or an admin with a written reason, proposes an outcome. The evidence must be in the evidence table. Proposing is allowed only for a closed market after its release time. A void proposal (the source missed the deadline) needs the void condition it satisfies.
2. **Confirm.** An admin other than the proposer confirms. Confirming starts `DISPUTE_WINDOW`: 24 hours by default, long enough for a person to notice a wrong outcome; tests set it shorter through configuration.
3. **Dispute.** Any user holding a position in the market may file one dispute per window, with a reason (rate-limited). An open dispute stops automatic settlement. An admin then either upholds it, which withdraws the proposal (a new one starts again at step 1), or rejects it, which lets settlement proceed. Every ruling is audit-logged with its reason.
4. **Settle.** When the window ends with no open dispute, the gateway's settler submits `Settle { market, outcome, evidence_hash }`.
- The one-step admin `settle` endpoint from G4 is removed. Settlement happens only through this flow (closing the "one step, not two" gap in the G4 threat model).

**Scheduler.** A gateway task opens and closes markets at their rulebook times, by sequenced commands audited as actor `system`. After a restart it catches up: a market past its close time is closed at once, and never traded on.

### Resolvers (`services/src/brier_services/resolution/`)
Each provider is a small module: `fetch(rule) -> Response`, `parse(response, rule) -> Value | NotYetPublished`, and a shared `decide`. The providers:
- **FRED/ALFRED** (St. Louis Fed API; needs a free API key, see Credentials). Covers the FOMC target upper bound (`DFEDTARU`) and the 10-year Treasury yield (`DGS10`). The S&P 500 is excluded: its series is licensed from S&P Dow Jones Indices (`docs/data-licenses.md`). ALFRED vintages give the value as first released, which is how the "first release counts" policy is checked.
- **BLS public API.** Covers CPI-U (`CUUR0000SA0`) and the unemployment rate (`LNS14000000`). Version 1 needs no key.
- **NOAA NCEI Access Data Service** (GHCN-Daily). Covers the daily maximum temperature at a named station. Needs no token.

**Evidence record:**
- the request URL with any key removed;
- retrieval time;
- HTTP status;
- the SHA-256 of the raw body, which is kept under `var/evidence/<hash>`;
- the parsed value, the decision, and the resolver's version.

**Safety:**
- HTTPS only, to a fixed list of hosts.
- Responses are size-limited and parsed strictly.
- A value the parser cannot read means no proposal. A resolver never guesses an outcome.

**CLI:** `python -m brier_services.resolve` checks due markets and proposes through the gateway API with its signed key.

### Launch catalogue (`markets/`)
Five markets, one per SPEC §7 example:
- CPI 12-month change above a threshold;
- the FOMC upper bound after a meeting;
- the unemployment rate;
- the 10-year Treasury yield above a level on a date (in place of the S&P 500; O3, O6);
- the Central Park daily maximum temperature.

Each one names its source, release time, and void conditions. O3 is settled in `docs/data-licenses.md`: raw BLS and NOAA responses are committed as fixtures; FRED responses stay local, and CI uses fixtures written by hand in FRED's format.

## Credentials
- FRED needs a free API key that only you can create, at fred.stlouisfed.org, under My Account and then API Keys. Put it in `.env` as `FRED_API_KEY`. It is never committed, logged, or shown in chat; evidence records strip it.
- BLS v1 and NOAA's Access Data Service need no key.

## Registered study: resolver correctness against history (`docs/notes/resolution.md`, registered before any resolver exists)
- **R1 (values):** each resolver's values equal an independent comparator's (a different system or product, parsed by separate code) over fixed windows: 60 months of CPI and unemployment, every FOMC statement since 2019, and every day of 2025 for the 10-year yield and the Central Park maximum temperature. Refuted by one mismatch.
- **R2 (evidence determines the decision):** recomputing every decision from the stored raw bytes alone, offline, gives the same outcome. Refuted by one difference.
- Full design, comparators, and limits: `docs/notes/resolution.md`.

## Units
1. This plan; the registered note; O3: read each source's terms and record them in `docs/data-licenses.md`, including whether its raw responses may be committed as test fixtures. Where they may not, fixtures keep only hashes plus a minimal excerpt, or are re-fetched locally.
2. Rulebook schema, validator, and RFC 8785 canonicalization in Rust and Python, with shared vectors and property tests (canonicalizing twice changes nothing; reordering keys gives the same hash; the timing rules).
3. Journal v3: `rules_hash` and `evidence_hash` through the engine, wire, protocol, reference engine, generator, and fuzz targets. The differential and crash suites still pass.
4. Gateway: the rulebook and rules endpoints, the resolver role, propose, confirm, dispute, rule, the settler, and the scheduler; Postgres migrations; removal of one-step settle; OpenAPI; audit.
5. Resolvers: FRED/ALFRED, BLS, NOAA, the evidence store, and the CLI, tested on recorded responses (malformed, truncated, not-yet-published, revised).
6. Launch catalogue and the exit test. For each market: list, trade, close by schedule, resolve from a recorded response, confirm, dispute and reject, settle. Checked: payouts, zero escrow, hashes in the journal equal to the rules and evidence, and the read models.
7. R1/R2 study, the G5 threat-model section (source spoofing, wrong resolution, rulebook tampering, admin collusion, trading after release), the dispute runbook, and the gate review.
