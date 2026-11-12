# Runbook: the public deployment

## Where Brier runs
Two Oracle Cloud Always Free micro servers (VM.Standard.E2.1.Micro: 1 GB of memory, 2 GB of swap each) in Ashburn. Both are $0 for the life of the account.

| Server | Private IP | Runs |
|---|---|---|
| **brier-db** | 10.0.0.138 | PostgreSQL 16, `brier-gateway`, and `brier-operator` (the lister, market maker, resolver and surveillance) |
| **brier-web** | 10.0.0.175 | `brier-web` (Next.js) behind Caddy |

- **Public address:** `https://132-145-162-222.sslip.io`. sslip.io maps the name to brier-web's public IP, so there is no domain to buy; Caddy gets a certificate from Let's Encrypt.
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
   gh workflow run release.yml -f site=https://132-145-162-222.sslip.io
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
curl https://132-145-162-222.sslip.io/v1/health
```
