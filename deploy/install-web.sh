#!/bin/bash
# Install or upgrade the public host (brier-web): Node, Caddy (HTTPS), and the web app.
#   sudo BRIER_HOST=example.org BRIER_GATEWAY=10.0.0.138:8080 BRIER_PROXY_KEY=... \
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
apt-get -o DPkg::Lock::Timeout=900 install -y -qq curl gnupg debian-keyring debian-archive-keyring apt-transport-https netfilter-persistent >/dev/null

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
install -d -o caddy -g caddy /var/log/caddy
install -m 644 "$STAGE"/deploy/systemd/brier-web.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable -q brier-web caddy
systemctl restart brier-web caddy
rm -rf "$STAGE"
echo "brier-web: $(cat /opt/brier/COMMIT) at https://$BRIER_HOST"
