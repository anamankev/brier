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
| Administration | Admin misuse | Admin role checked on every request; a reason (3-500 characters) is required; the audit row is written before the command is submitted and completed with its sequence number and the engine's answer; promotion only through the operator CLI, which also audits | `rest.rs` audit count and role cases | One step, not two (two-step resolution is G5). The database role can rewrite `audit_log`; it is not append-only. No admin second factor (G6) |
| Privacy | One user learning about another's activity | Other accounts' orders answer `unknown_order` or 404; order replies show only the caller's fills; public trades and the trades channel carry no account identities | `rest.rs` and `ws.rs` cases | Registration reveals whether a username is taken (rate-limited, as on most exchanges) |
| Read models | History that is wrong or silently stale | Projected from the journal, one transaction per batch with its cursor; every history answer states `as_of` | `rebuild.rs` (live equals rebuild) | None known |
| Dependencies | A fresh, compromised release | Every locked version must have been public for 14 days: Python resolution has a fixed cutoff (`exclude-newer`), and `scripts/dependency_age.py` checks all three lockfiles against crates.io and PyPI whenever one changes. G4's first check found 5 Python dev tools and 16 transitive crates that broke the rule; all were rolled back | `services/pyproject.toml`, `dependency-age.yml` | Cargo has no resolution cutoff, so a `cargo update` can still pick a fresh crate; the check catches it at push time |

## Components to come (from SPEC §13), owned by their gates

| Gate | Threat | Planned mitigation |
|---|---|---|
| G1-G2 | Engine or ledger bug that creates or destroys credits | Invariants I1-I5 checked after every event; differential tests against the reference engine; halt on violation |
| G3 | Acknowledged order lost in a crash; corrupt journal | Sync before acknowledgment (F_FULLFSYNC on macOS); CRC32C per record; SIGKILL crash harness |
| G5 | Wrong or manipulated resolution | Recorded source evidence; two-step admin confirmation; dispute window; audit log |
| G6 | Account takeover | TOTP second factor for people and admins |
| G5-G6 | Admin misuse | Every admin action is a sequenced, audit-logged command; least-privilege roles |
| G8 | Multi-accounting and collusion | Account-linkage signals in surveillance; credits have no value, which limits the incentive |
