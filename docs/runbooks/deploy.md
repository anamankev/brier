# Runbook: the public deployment

## Where Brier runs
Two Oracle Cloud Always Free micro servers (VM.Standard.E2.1.Micro: 1 GB of memory, 2 GB of swap each) in Ashburn. Both are $0 for the life of the account.

| Server | Private IP | Runs |
|---|---|---|
| **brier-db** | 10.0.0.138 | PostgreSQL 16, `brier-gateway`, and `brier-operator` (the lister, market maker, resolver and surveillance) |
| **brier-web** | 10.0.0.175 | `brier-web` (Next.js) behind Caddy |

- **Public address:** `https://playbrier.duckdns.org`, a free DuckDNS name that points to brier-web's public IP (set on duckdns.org by the owner). Caddy gets the certificate from Let's Encrypt.
- **Former address:** `https://132-145-162-222.sslip.io` (sslip.io maps the name to the IP) forwards there permanently, path and query kept, so old links keep working. Former addresses are listed in `deploy/old-hosts`.
- **Planned address:** `brier.eu.org`, requested at nic.eu.org. When it is approved, build the release with it as the site and add `playbrier.duckdns.org` to `deploy/old-hosts`.
- **Caddy routing:** `/v1/*` (the REST API and the WebSocket) goes to the gateway, carrying the proxy key and the client's address; everything else goes to the web app.
- **Firewall:** Oracle's security list admits 22, 80 and 443 from anywhere, and everything inside 10.0.0.0/16. The gateway (8080) is reachable only on the private network.

## Access
- `ssh brier-db` reaches brier-db on **port 443**, because the owner's network blocks outbound 22.
- `ssh brier-web` goes through brier-db (ProxyJump).
- Both use the key `~/.ssh/brier_oracle`.
- The host entries live in `~/.ssh/config` on the owner's Mac.

## Secrets (never in git)
| Where | What |
|---|---|
| brier-db `/etc/brier/gateway.env` | Database URL, TOTP key, proxy key |
| brier-db `/etc/brier/operator.env` | Database URL; FRED, balldontlie and football-data keys |
| brier-db `/etc/brier/backup.key` | Backup encryption key; a copy is in `~/.oci/brier/backup.key` on the owner's Mac |
| brier-db `/var/lib/brier/operator/accounts.json` | Service-account password |
| brier-web `/etc/brier/web.env` and `/etc/brier/caddy.env` | Proxy key and hostnames |

All these files are mode 600 or 640, and every secret was generated on the server.

## Deploy a new version
1. Push to main and wait for `check` to pass.
2. Build the release:
   ```bash
   gh workflow run release.yml -f site=https://playbrier.duckdns.org
   ```
3. Note the run ID:
   ```bash
   gh run list --workflow release.yml -L 1
   ```
4. Deploy:
   ```bash
   deploy/deploy.sh <run-id> both
   ```
   Use `db` or `web` instead of `both` to deploy one side. Installs run as systemd jobs, so a dropped connection cannot leave a server half-installed. The script ends by printing three health checks.

## Backups
- **Schedule:** nightly at 03:30 UTC, from `brier-backup.timer` on brier-db.
- **Contents:** a `pg_dump` and the journal, encrypted with AES-256 (PBKDF2, 200,000 iterations).
- **Where they go:** over the private network to brier-web, `/var/backups/brier`. The SSH key brier-db uses can only run the receive command. brier-web deletes backups older than 30 days.
- **Run one now:**
  ```bash
  ssh brier-db sudo systemctl start brier-backup
  ```

## Restore
1. Copy a backup from brier-web, then decrypt it:
   ```bash
   openssl enc -d -aes-256-cbc -pbkdf2 -iter 200000 -pass file:backup.key -in brier-....tar.enc | tar -xf -
   ```
2. Stop the operator, then the gateway:
   ```bash
   sudo systemctl stop brier-operator brier-gateway
   ```
3. Restore the database:
   ```bash
   sudo -u postgres pg_restore --clean --if-exists -d brier brier.pgdump
   ```
4. Replace `/var/lib/brier/journal` with the one from `journal.tar`.
5. Start the gateway, then the operator. The gateway replays the journal and the projector catches up; `/v1/health` shows both sequence numbers.

## Rotate secrets
Generate every new value on the server, for example `openssl rand -hex 32`, and never paste one into chat, a commit or a log.
- **Proxy key:**
  1. Replace `BRIER_PROXY_KEY` in brier-db `/etc/brier/gateway.env`.
  2. Restart the gateway:
     ```bash
     ssh brier-db sudo systemctl restart brier-gateway
     ```
  3. Redeploy the web side with `deploy/deploy.sh <run-id> web`. It rewrites `web.env` and `caddy.env` from the gateway's key.
- **Data-source keys** (FRED, balldontlie, football-data):
  1. Make a new key on the provider's site and revoke the old one.
  2. Replace the key in `/etc/brier/operator.env`.
  3. Restart the operator:
     ```bash
     ssh brier-db sudo systemctl restart brier-operator
     ```
- **Database password:**
  1. Run `ALTER ROLE brier PASSWORD '...'` as `postgres`.
  2. Update the URL in both `gateway.env` and `operator.env`.
  3. Restart the gateway, then the operator.
- **Backup key:**
  1. Write a new `/etc/brier/backup.key`.
  2. Keep the old key beside the old backups until they age out (30 days), or they cannot be restored.
  3. Update the copy on the owner's Mac.
- **TOTP key:** rotate only if it leaks. Every TOTP secret is encrypted under it, so after a change every user with two-step sign-in, admins included, must turn it off with a recovery code and enrol again.

## Uptime alert
The `uptime` workflow runs on GitHub Actions every 15 minutes. It fails when the site, the exchange's health check, or the events list does not answer, or when the projector falls more than 100 commands behind the journal. A failed scheduled run emails the repository owner. Run it by hand with:
```bash
gh workflow run uptime.yml
```

## Halt trading
- **From the admin console:** an admin signed in with TOTP uses the kill switches (halt all, cancel all).
- **Or on the server:**
  ```bash
  ssh brier-db sudo systemctl stop brier-operator brier-gateway
  ```
  The site stays up and shows that the markets are unavailable.

## Logs and status
```bash
ssh brier-db sudo journalctl -u brier-gateway -u brier-operator -f
```
```bash
ssh brier-web sudo journalctl -u brier-web -u caddy -f
```
```bash
curl https://playbrier.duckdns.org/v1/health
```

## Drill record
| Date | Drill | Result |
|---|---|---|
| 2026-11-11 | Restore: the latest nightly backup was decrypted and restored into a scratch database and journal, then a second gateway was started on them | Passed. The restored gateway reached seq 1695 with 80 events, the same as live. The decrypted copies were deleted afterwards. |
| 2026-11-11 | Restart: both servers rebooted | Passed. The site answered again after 73 s and the exchange after 79 s; every service started on its own. |
| 2026-11-11 | Load test (`scripts/load_test.py`, see `docs/notes/load-test.md`) | About 14 pages a second at 50 users, no errors. |
| 2026-11-12 | Move to playbrier.duckdns.org | The redirect file for the old address was written mode 600 (the installer's umask), Caddy could not read it and stopped: the site was down for about 2 minutes. Fixed on the server, and the installer now writes it readable. The new certificate came from Let's Encrypt in 5 seconds. |
| 2026-11-12 | Deploy during an automatic security update | The install stopped on the package lock while `deploy.sh` still reported success. Fixed: installs now wait up to 15 minutes for the lock, and a failed install fails the deploy. |
