# G8 review: surveillance

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| The hypotheses' commit predates the results | MET | The note was registered at `26cd6c3`. Two dated amendments followed, both before any simulation ran: `fa21561` and `0e2854b`. The simulation and runner were committed at `bc10435`, then run. The results were committed at `5e2651d` |
| Precision and recall reported per typology | MET | `docs/notes/surveillance.md` §4: test-set precision, recall, and 95% bootstrap intervals for all four typologies, with false positives broken down by account kind |

## Registered results: two supported, two not
| | Typology | Precision | Recall | Verdict |
|---|---|---|---|---|
| V1 | Wash trading | 1.000 | 0.979 | Supported |
| V2 | Spoofing | 0.296 | 0.933 | Not supported |
| V3 | Marking the close | 0.535 | 0.701 | Not supported |
| V4 | Insider-like | 1.000 | 0.990 | Supported |

- **Spoofing failed** because the registered score added up a large passive trader's unpaired cancels until they outranked the spoofer's paired ones.
- **An exploratory rule fixed it.** On fresh seeds 301-500, counting only orders paired with an opposite trade *the account initiated* reached precision 1.00 and recall 0.99. This was chosen after seeing the results, so it is reported as exploratory, with the caveat that it fits how the look-alike was built. It is the rule the live service runs.
- **Marking the close failed** because a late, informed trader moves the last price just as a manipulator does. The note proposes the fix (what happens after the push) as a new registration.

## What was built
- **`brier-core export`** turns the journal back into wire commands.
  - `brier_wire::command_json` is the property-tested inverse of `parse_command`.
  - An exported journal replays to the verified state hash.
- **The tape and the four detectors** (`brier_services.surveillance`).
  - The tape is built on each market's own command clock.
  - The detectors are unit-tested on hand-built streams.
- **The simulation** (`brier_services.sim.surveillance`):
  - four injected manipulators;
  - four innocent look-alikes;
  - seeded and reproducible.
- **Login signals:** keyed address hashes. Forwarded addresses are trusted only with a shared proxy key, which also gives web users real per-address rate limits once an edge proxy is configured.
- **The `surveillance` role:** it opens cases and reads links, and cannot trade, forecast, or close a case.
- **Case endpoints** with audited closing.
- **The live service** and its CLI.
- **The admin case list and case page:**
  - the detector's numbers;
  - P&L in the market;
  - linked accounts;
  - the account's activity, with the rows the alert names highlighted;
  - closing with a reason.
- **The demo exchange** now includes a wash pair and a spoofer, and runs surveillance once.
  - The pass opened exactly three cases: both wash accounts and the spoofer, with no false alarms among the makers and takers.
  - The case pages were reviewed in a browser at desktop and phone widths: no horizontal scroll, and closing a case was recorded with its reason.

## Found during G8, and fixed
1. **The web server hid every client's address.** All web logins and requests reached the gateway from the web server's own address. Two consequences:
   - linking would have tied every web user together;
   - the per-address rate limit was effectively one bucket for all web users.

   Next.js sets `x-forwarded-for` only when the client did not send one, so that header cannot be trusted. The fix is the keyed proxy header, recorded in the threat model.
2. **A zero spoofing score could be emitted** when the unpaired weight was zero. Zero scores are never emitted now. The registered numbers were unchanged, and the rerun confirmed it.
3. **The demo's spoofer missed the bid twice** because it priced from a stale fair value rather than the book. The case page's timeline showed it ("unfilled rest cancelled"), and the detector correctly counted only the one paired episode. The demo now prices from the book.
4. **Two test expectations were wrong, not the code.**
   - A market's clock counts its own listing.
   - When NO wins, the seller is the one positioned on the winning side.

## Deviations and gaps
- **Evaluation in simulation only.** Real manipulators adapt; the detectors and the manipulators were written by the same author to a published specification.
- **Wash trading through unlinked accounts is invisible.** Linking needs a shared address, and without an edge proxy header, web logins record no address at all.
- **Marking the close stays weak** (precision 0.54). Its cases are leads to check by hand.
- **No second-admin review of case closures**, unlike resolutions.
- **The case pages are not in the Playwright suite yet.** The browser test stack has no surveillance pass; they were checked by hand. G9's browser work adds them.

## Compared with Kalshi and Polymarket
- **Kalshi** is a CFTC-regulated exchange with a surveillance obligation, staff, and its exchange's full order data. Its detectors and their accuracy are not public.
- **Polymarket's** order book is public on-chain, and outside analysts study it for wash trading. The venue publishes no detector accuracy.
- **Brier** publishes its detectors, the registered test, two failures, and the evidence behind every case. That is the engineering point of this gate.

## Exam questions
1. The registered spoofing score failed against a passive trader. Derive the large passive trader's expected registered score per market from its parameters, and show why no threshold separates it from a spoofer with 3-6 episodes.
2. The live spoofing rule counts only opposite trades the account initiated. Describe a spoofer who defeats it, and the cost to that spoofer.
3. Linking stores an HMAC of the address under a key derived from the TOTP key. What does an attacker with a database dump but no key learn? With the key? Should the two keys be separate?
4. The export can include a command from a batch not yet synced. Construct the sequence in which surveillance opens a case about an order that, after a crash, never happened, and say how the case page should show it.
