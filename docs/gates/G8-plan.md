# G8 plan: surveillance

Reads: SPEC §9, §10, §13, §0 item 7. Exit: the hypotheses' commit predates the results; precision and recall are reported per typology.

## The problem
Regulated venues must catch manipulation; prediction markets are now being held to that standard (in 2026 the CFTC brought its first insider-trading cases in prediction markets). Brier needs detectors for the four typologies in SPEC §9, cases an admin can work, and an honest measurement of how well the detectors work. The measurement is the point: anyone can write a rule, and few venues publish how often it is right.

## One event stream for live and simulated markets
- **Live:** `brier-core export --data DIR` writes the journal back out as wire commands (JSON lines), reading it without changing it. The Python reference engine replays those commands into events, with accounts on every order and trade.
- **Simulated:** the harness produces the same commands and events directly.
- The detectors read only the stream (sequence number, command, events), plus account metadata: creation order and linked-account signals. They cannot tell a simulated market from a live one, so the evaluation measures the code that runs live.
- **Time** is measured in sequence numbers and, in simulation, steps. Journal commands carry no wall-clock time (protocol §8). "The final window before close" is the commands before the market's `set_state closed`. Every launch market closes before its release time, so a position taken before the close is a position taken before the release.

## Linked accounts
- The gateway records, at each login, a keyed hash of the client address: HMAC-SHA-256 under a key derived from `BRIER_TOTP_KEY` with its own label. The address itself is never stored.
- Two accounts are linked when they share a hash.
- **The weakness:** an address is a weak signal. Households and offices share addresses, and VPNs hide them. The evaluation includes innocent linked accounts for exactly this reason.

## Detectors (`brier_services/surveillance/`)
Each detector gives a score per (account, market). An alert is a score at or above the detector's threshold, and each alert opens a case with its evidence.
- **Wash trading:** contracts traded between linked accounts, as a share of each account's volume in the market (same-account trades cannot happen: self-trade prevention).
- **Spoofing and layering:** orders at least k times the median resting size, within two ticks of the touch, cancelled unfilled within w commands. The score counts such orders, weighted by how often the same account trades on the other side within w commands of each cancel.
- **Marking the close:** in the last n commands before the close, the account's share of taker volume, times the move in the last trade price it caused.
- **Insider-like trading:** a net position opened in the last n commands before the close, large against the account's earlier activity, and on the side that wins. Weighted up when the account is new or linked.

## Cases and the admin view
- **Tables:** `cases` (typology, market, accounts, score, evidence, status, timestamps) and `login_signals` (user, keyed address hash, first and last seen).
- **The surveillance service** (`python -m brier_services.surveillance run`):
  - exports the journal;
  - runs the detectors;
  - opens a case for each new alert through an admin-only endpoint, signed with its own admin key.
- **The admin console:** a list of cases and a case page with the evidence timeline (the account's orders, cancels and fills around the alert, the book at the time, P&L), and closing a case with a reason, which is audited.

## Registered evaluation (`docs/notes/surveillance.md`, before any detector exists)
- **Data:** seeded simulated markets with manipulators injected as ground truth, among innocent traders, some built to look suspicious.
- **Tuning and test sets:** thresholds are chosen on tuning seeds; precision and recall are reported on separate test seeds, with bootstrap confidence intervals over seeds.

## Units
1. This plan and the registered note.
2. `brier-core export` and `brier_wire::command_json` (the inverse of `parse_command`), with round-trip property tests and a check that an exported journal replays to the same state hash.
3. The detector library and its unit tests on hand-built streams.
4. Manipulators and innocent look-alikes in the simulation harness.
5. The evaluation: tune, then test, then report.
6. Cases: login signals, tables and endpoints, the surveillance service, and the admin case view in the web app.
7. The G8 threat-model section and the gate review.
