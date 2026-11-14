#!/bin/bash
# Install or upgrade the public host (brier-web): Node, Caddy (HTTPS), and the web app.
#   sudo BRIER_HOST=example.org BRIER_GATEWAY=10.0.0.138:8080 BRIER_PROXY_KEY=... \
#   (optional: BRIER_OLD_HOSTS="old.example.org" to forward former addresses) \
#        bash install-web.sh /path/to/brier-release.tar.gz
set -euo pipefail
RELEASE=${1:?usage: install-web.sh brier-release.tar.gz}
: "${BRIER_HOST:?}" "${BRIER_GATEWAY:?}" "${BRIER_PROXY_KEY:?}"

if ! swapon --show | grep -q /swapfile; then
  fallocate -l 2G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile
  grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi
export DEBIAN_FRONTEND=noninteractive
apt-get -o DPkg::Lock::Timeout=900 update -qq
apt-get -o DPkg::Lock::Timeout=900 install -y -qq curl gnupg debian-keyring debian-archive-keyring apt-transport-https netfilter-persistent python3-venv >/dev/null

# Node 24 from nodejs.org: the newest 24.x released at least 14 days ago, checked against its
# published SHA-256.
if ! node --version 2>/dev/null | grep -q '^v24\.'; then
  V=$(curl -fsS https://nodejs.org/dist/index.json | python3 -c '
import json, sys, datetime as dt
cut = dt.date.today() - dt.timedelta(days=14)
print(next(r["version"] for r in json.load(sys.stdin) if r["version"].startswith("v24.") and dt.date.fromisoformat(r["date"]) <= cut))')
  T=$(mktemp -d)
  curl -fsSo "$T/node.tar.xz" "https://nodejs.org/dist/$V/node-$V-linux-x64.tar.xz"
  curl -fsSo "$T/SHASUMS256.txt" "https://nodejs.org/dist/$V/SHASUMS256.txt"
  (cd "$T" && grep " node-$V-linux-x64.tar.xz\$" SHASUMS256.txt | sed "s/node-$V-linux-x64.tar.xz/node.tar.xz/" | sha256sum -c -)
  tar -xJf "$T/node.tar.xz" -C /usr/local --strip-components=1
  ln -sf /usr/local/bin/node /usr/bin/node
  rm -rf "$T"
fi

# Caddy from its official repository.
if ! command -v caddy >/dev/null; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' > /etc/apt/sources.list.d/caddy-stable.list
  apt-get -o DPkg::Lock::Timeout=900 update -qq && apt-get -o DPkg::Lock::Timeout=900 install -y -qq caddy >/dev/null
fi

# The firewall: web traffic in.
for p in 80 443; do iptables -C INPUT -p tcp --dport $p -j ACCEPT 2>/dev/null || iptables -I INPUT -p tcp --dport $p -j ACCEPT; done
# SSH only from the private network: the operator reaches this server through brier-db.
iptables -C INPUT -s 10.0.0.0/16 -p tcp --dport 22 -j ACCEPT 2>/dev/null || iptables -I INPUT -s 10.0.0.0/16 -p tcp --dport 22 -j ACCEPT
while iptables -C INPUT -p tcp -m state --state NEW -m tcp --dport 22 -j ACCEPT 2>/dev/null; do
  iptables -D INPUT -p tcp -m state --state NEW -m tcp --dport 22 -j ACCEPT
done
netfilter-persistent save >/dev/null

id brier >/dev/null 2>&1 || useradd --system --home /opt/brier --shell /usr/sbin/nologin brier
install -d -m 755 /opt/brier
install -d -m 755 /etc/brier
STAGE=$(mktemp -d)
tar -xzf "$RELEASE" -C "$STAGE"
rm -rf /opt/brier/web.new && cp -r "$STAGE"/web /opt/brier/web.new && chown -R root:root /opt/brier/web.new
rm -rf /opt/brier/web.old; [ -d /opt/brier/web ] && mv /opt/brier/web /opt/brier/web.old; mv /opt/brier/web.new /opt/brier/web
cp "$STAGE"/COMMIT /opt/brier/COMMIT

umask 077
cat > /etc/brier/web.env <<ENV
NODE_ENV=production
PORT=3000
HOSTNAME=127.0.0.1
BRIER_GATEWAY_URL=http://$BRIER_GATEWAY
BRIER_WS_URL=wss://$BRIER_HOST/v1/ws
BRIER_SITE_URL=https://$BRIER_HOST
BRIER_PROXY_KEY=$BRIER_PROXY_KEY
BRIER_CLIENT_ADDRESS_HEADER=x-forwarded-for
ENV
cat > /etc/brier/caddy.env <<ENV
BRIER_HOST=$BRIER_HOST
BRIER_GATEWAY=$BRIER_GATEWAY
BRIER_PROXY_KEY=$BRIER_PROXY_KEY
ENV
chown root:brier /etc/brier/web.env && chmod 640 /etc/brier/web.env
chown root:caddy /etc/brier/caddy.env && chmod 640 /etc/brier/caddy.env
install -d /etc/systemd/system/caddy.service.d
printf '[Service]\nEnvironmentFile=/etc/brier/caddy.env\n' > /etc/systemd/system/caddy.service.d/brier.conf
install -m 644 "$STAGE"/deploy/Caddyfile /etc/caddy/Caddyfile
# Addresses the site used to have (deploy/old-hosts, or BRIER_OLD_HOSTS) answer with a
# permanent redirect to the current one.
rm -rf /etc/caddy/brier-redirects && install -d -m 755 /etc/caddy/brier-redirects
BRIER_OLD_HOSTS=${BRIER_OLD_HOSTS:-$(grep -v '^#' "$STAGE"/deploy/old-hosts 2>/dev/null | grep -vx "$BRIER_HOST" | tr '\n' ' ')}
BRIER_OLD_HOSTS=$(echo $BRIER_OLD_HOSTS)
if [ -n "${BRIER_OLD_HOSTS:-}" ]; then
  printf '%s {\n\tredir https://%s{uri} permanent\n}\n' "$(echo $BRIER_OLD_HOSTS | sed 's/ /, /g')" "$BRIER_HOST" \
    > /etc/caddy/brier-redirects/old-hosts.caddy
  chmod 644 /etc/caddy/brier-redirects/old-hosts.caddy  # Caddy runs as its own user; umask is 077 here
fi
caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1 || { echo "Caddyfile does not validate" >&2; exit 1; }
install -d -o caddy -g caddy /var/log/caddy
# The bots (lister, market maker, resolver): the Python services from the release, run against
# the gateway over the private network. Their keys come from brier-db through deploy.sh.
(
  umask 022  # the service user must read the code; the 077 above is for the secret files
  [ -d /opt/brier/venv ] || python3 -m venv /opt/brier/venv
  /opt/brier/venv/bin/pip install -q --require-hashes -r "$STAGE"/py/requirements.txt
  /opt/brier/venv/bin/pip install -q --no-deps --force-reinstall "$STAGE"/py/*.whl
)
install -d -o brier -g brier -m 750 /var/lib/brier /var/lib/brier/operator
if [ -f /tmp/operator-install.env ]; then
  install -o root -g brier -m 640 /tmp/operator-install.env /etc/brier/operator.env
  rm -f /tmp/operator-install.env
fi
install -m 644 "$STAGE"/deploy/systemd/brier-bots.service /etc/systemd/system/
install -m 644 "$STAGE"/deploy/systemd/brier-web.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable -q brier-web caddy
systemctl restart brier-web caddy
# The bots start once their saved accounts are here (moved from brier-db once; runbook).
if [ -f /var/lib/brier/operator/accounts.json ]; then systemctl enable -q brier-bots; systemctl restart brier-bots; fi
# SSH: keys only, no root login, no X11 (deploy/sshd-hardening.conf). Checked before reloading,
# so a bad file never locks the operator out.
install -m 644 "$STAGE"/deploy/sshd-hardening.conf /etc/ssh/sshd_config.d/10-brier.conf
sshd -t && systemctl reload ssh
# System and security logs are kept 30 days (the privacy policy says so).
install -d -m 755 /etc/systemd/journald.conf.d
printf '[Journal]\nMaxRetentionSec=30day\n' > /etc/systemd/journald.conf.d/10-brier.conf
chmod 644 /etc/systemd/journald.conf.d/10-brier.conf
systemctl restart systemd-journald
# Nightly backup of the bots' state to the private GitHub repository (needs the backup key).
install -m 755 "$STAGE"/deploy/github-backup.sh /usr/local/bin/brier-github-backup
install -m 755 "$STAGE"/deploy/bots-backup.sh /usr/local/bin/brier-bots-backup
install -m 644 "$STAGE"/deploy/systemd/brier-bots-backup.service "$STAGE"/deploy/systemd/brier-bots-backup.timer /etc/systemd/system/
systemctl daemon-reload
if [ -f /etc/brier/backup.key ]; then systemctl enable -q --now brier-bots-backup.timer; fi
rm -rf "$STAGE"
echo "brier-web: $(cat /opt/brier/COMMIT) at https://$BRIER_HOST"
