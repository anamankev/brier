#!/bin/bash
# Nightly backup on brier-db (deploy/install-backup.sh installs it): a PostgreSQL dump and the
# journal, encrypted with the key in /etc/brier/backup.key, sent to brier-web over the private
# network. brier-web keeps 30 days (the privacy notice's promise).
set -euo pipefail
. /etc/brier/backup.env   # BACKUP_TARGET (user@private-ip)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
sudo -u postgres pg_dump --format=custom brier > "$TMP/brier.pgdump"
tar -C /var/lib/brier -cf "$TMP/journal.tar" journal
tar -C "$TMP" -cf - brier.pgdump journal.tar \
  | openssl enc -aes-256-cbc -pbkdf2 -iter 200000 -salt -pass file:/etc/brier/backup.key \
  | ssh -i /etc/brier/backup_ed25519 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/etc/brier/backup_known_hosts \
      "$BACKUP_TARGET" "brier-$STAMP.tar.enc"
echo "backup brier-$STAMP.tar.enc sent"
