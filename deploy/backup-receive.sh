#!/bin/bash
# The only command brier-db's backup key may run on brier-web: store stdin under a checked name
# and delete backups older than 30 days.
set -euo pipefail
NAME=${SSH_ORIGINAL_COMMAND:-}
[[ "$NAME" =~ ^brier-[0-9]{8}T[0-9]{6}Z\.tar\.enc$ ]] || { echo "refused" >&2; exit 1; }
cat > "/var/backups/brier/$NAME.part"
mv "/var/backups/brier/$NAME.part" "/var/backups/brier/$NAME"
find /var/backups/brier -name 'brier-*.tar.enc' -mtime +30 -delete
