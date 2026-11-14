#!/bin/bash
# Deploy a release built by the `release` workflow to both servers (docs/runbooks/deploy.md).
#   deploy/deploy.sh <run-id> [db|web|both]
#   BRIER_OLD_HOSTS="old.example.org" deploy/deploy.sh ...   (former addresses redirect to the new one)
# Needs the `brier-db` and `brier-web` SSH host entries. The proxy key goes from server to
# server through this shell and is never printed or written locally.
set -euo pipefail
RUN=${1:?usage: deploy.sh <release run id> [db|web|both]}
WHICH=${2:-both}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIR="$ROOT/var/release/$RUN"
rm -rf "$DIR" && mkdir -p "$DIR"
gh run download "$RUN" -n brier-release -D "$DIR"
TAR="$DIR/brier-release.tar.gz"
SITE=$(tar -xzf "$TAR" -O ./SITE)
HOST=${SITE#https://}

detached() { # host, script, log: run an install as a systemd job so a dropped connection cannot stop it
  # (--wait returns the job's exit status, kept past the tail, so a failed install fails the deploy)
  ssh "$1" "sudo systemd-run --unit=brier-install --collect --wait bash -c '$2 > $3 2>&1'; s=\$?; tail -3 $3; exit \$s" \
    || { echo "install on $1 failed (log: $3)" >&2; exit 1; }
}

if [ "$WHICH" != web ]; then
  scp -q "$TAR" brier-db:/tmp/brier-release.tar.gz
  tar -xzf "$TAR" -O ./deploy/install-db.sh | ssh brier-db 'cat > /tmp/install-db.sh'
  detached brier-db "bash /tmp/install-db.sh /tmp/brier-release.tar.gz" /tmp/install-db.log
fi
if [ "$WHICH" != db ]; then
  GATEWAY="$(ssh brier-db "hostname -I | awk '{print \$1}'"):8080"
  scp -q "$TAR" brier-web:/tmp/brier-release.tar.gz
  tar -xzf "$TAR" -O ./deploy/install-web.sh | ssh brier-web 'cat > /tmp/install-web.sh'
  ssh brier-db 'sudo grep ^BRIER_PROXY_KEY= /etc/brier/gateway.env' \
    | { cat; printf 'BRIER_HOST=%s\nBRIER_GATEWAY=%s\nBRIER_OLD_HOSTS="%s"\n' "$HOST" "$GATEWAY" "${BRIER_OLD_HOSTS:-}"; } \
    | ssh brier-web 'umask 077; cat > /tmp/web-install.env'
  # The bots' data-source keys, from brier-db's operator.env (never printed or kept locally).
  ssh brier-db 'sudo grep -E "^(FRED_API_KEY|BALLDONTLIE_API_KEY|FOOTBALL_DATA_API_KEY|BRIER_SPORTS_CACHE)=" /etc/brier/operator.env' \
    | ssh brier-web 'umask 077; cat > /tmp/operator-install.env'
  detached brier-web "set -a; . /tmp/web-install.env; set +a; rm -f /tmp/web-install.env; bash /tmp/install-web.sh /tmp/brier-release.tar.gz" /tmp/install-web.log
fi
for path in /v1/health / /photos/nyc.webp; do
  code=000
  for _ in $(seq 1 30); do  # the web app takes a few seconds to start
    code=$(curl -s -o /dev/null -m 30 -w '%{http_code}' "$SITE$path")
    [ "$code" = 200 ] && break
    sleep 2
  done
  printf '%s %s\n' "$path" "$code"
done
