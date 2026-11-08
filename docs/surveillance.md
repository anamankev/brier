# Surveillance

How Brier watches its own markets for manipulation (SPEC §9). The evaluation is in `docs/notes/surveillance.md`; the plan is `docs/gates/G8-plan.md`.

## The pieces
1. **Login signals.**
   - At each login, the gateway stores an HMAC-SHA-256 of the client address, never the address itself.
   - Two accounts that share a hash are linked: weak evidence, since households share addresses.
   - Behind the web server, the gateway trusts the forwarded address only with `BRIER_PROXY_KEY`. The web server forwards the header named by `BRIER_CLIENT_ADDRESS_HEADER`, which an edge proxy must overwrite.
2. **The export.** `brier-core export --data DIR` writes the journal out as wire commands without modifying it. The reference engine replays them into the tape: every order, cancel, and fill, with the book before each order.
3. **The detectors** (`brier_services.surveillance.detectors`): wash trading, spoofing, marking the close, and insider-like trading. Each gives a score per account and market.
4. **The service** (`brier_services.surveillance.service`):
   - It runs as a user with the `surveillance` role and a signed key.
   - It applies the thresholds chosen in the evaluation, and the spoofing rule measured in §5 of the note.
   - It opens one case per alert, with the detector's numbers, the account's activity in the market, and its P&L there.
5. **Cases** (`/admin/cases` in the web app):
   - An admin signed in with TOTP reads the evidence and the linked accounts.
   - The admin closes each case as dismissed or actioned, with a reason that goes to the audit log.

## Running it
```bash
brier-gateway role watch surveillance "surveillance service"
python -m brier_services.resolve keygen --key-file var/watch.key   # sign in as watch and register the printed public key (POST /v1/keys)
python -m brier_services.surveillance run --gateway URL --key-id N --key-file var/watch.key --data DIR --core target/release/brier-core
```

Each pass reads the whole journal. Run it after each settlement: insider-like trading is judged only against the winning side. `--every SECONDS` repeats it.

## What an alert means
A case is a lead for a person, not a finding:
- marking the close catches about 70% of simulated closers at precision 0.54;
- an informed trader who is right late looks like an insider;
- a household on one network looks like a wash pair.

The evidence page shows the raw activity so that the admin decides, not the score.
