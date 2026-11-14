#!/bin/bash
# Off-site copy of an encrypted backup: brier-github-backup <branch> <file> (as root).
# Keeps the newest files of the branch in /var/lib/brier-github/<branch>, commits them as one
# orphan commit and force-pushes it to the private repository, so GitHub holds two weeks of
# backups and no history. The files are already encrypted; the key is never pushed.
set -euo pipefail
BRANCH=$1
FILE=$2
KEEP=${BRIER_BACKUP_KEEP:-14}
REPO=git@github.com:anamankev/brier-backups.git
KEY=/etc/brier/github_backup_ed25519
[ -f "$KEY" ] || { echo "no GitHub backup key; off-site copy skipped"; exit 0; }
DIR=/var/lib/brier-github/$BRANCH
install -d -m 700 "$DIR"
cp "$FILE" "$DIR"/
ls -1t "$DIR"/*.tar.enc | tail -n +$((KEEP + 1)) | xargs -r rm -f
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
cp "$DIR"/*.tar.enc "$WORK"/
cat > "$WORK/README.md" <<EOF
Encrypted Brier backups ($BRANCH branch): the newest $KEEP, replaced nightly with no history.
AES-256-CBC with PBKDF2 (200,000 iterations). The key is not in this repository.
Restore: docs/runbooks/deploy.md in anamankev/brier.
EOF
git -C "$WORK" init -q -b "$BRANCH"
git -C "$WORK" add -A
git -C "$WORK" -c user.name="Brier backup" -c user.email="backup@brier.invalid" \
  commit -q -m "Backups as of $(date -u +%Y-%m-%dT%H:%MZ)"
GIT_SSH_COMMAND="ssh -i $KEY -o IdentitiesOnly=yes -o UserKnownHostsFile=/etc/brier/github_known_hosts -o StrictHostKeyChecking=yes" \
  git -C "$WORK" push -q --force "$REPO" "$BRANCH"
echo "off-site: $(ls "$WORK"/*.tar.enc | wc -l) backups on branch $BRANCH"
