#!/bin/bash
# Nightly backup on brier-web of the bots' state (/var/lib/brier/operator: the saved service
# accounts, the resolver's raw evidence, the sports ratings), encrypted with the same key as the
# exchange backups and pushed off-site (deploy/github-backup.sh, branch "bots").
set -euo pipefail
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
OUT="$TMP/bots-$STAMP.tar.enc"
tar -C /var/lib/brier -cf - operator \
  | openssl enc -aes-256-cbc -pbkdf2 -iter 200000 -salt -pass file:/etc/brier/backup.key > "$OUT"
/usr/local/bin/brier-github-backup bots "$OUT"
