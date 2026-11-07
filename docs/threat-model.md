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

## Components to come (from SPEC §13), owned by their gates

| Gate | Threat | Planned mitigation |
|---|---|---|
| G1-G2 | Engine or ledger bug that creates or destroys credits | Invariants I1-I5 checked after every event; differential tests against the reference engine; halt on violation |
| G3 | Acknowledged order lost in a crash; corrupt journal | Sync before acknowledgment (F_FULLFSYNC on macOS); CRC32C per record; SIGKILL crash harness |
| G4 | Forged or replayed orders | Ed25519-signed requests; timestamp window; nonces; idempotent client order IDs |
| G4 | API abuse, denial of service | Per-account and per-IP token buckets; payload size limits; fuzzed parsers |
| G4 | Account takeover | Argon2id password hashing; optional TOTP; session expiry |
| G5 | Wrong or manipulated resolution | Recorded source evidence; two-step admin confirmation; dispute window; audit log |
| G5-G6 | Admin misuse | Every admin action is a sequenced, audit-logged command; least-privilege roles |
| G8 | Multi-accounting and collusion | Account-linkage signals in surveillance; credits have no value, which limits the incentive |
