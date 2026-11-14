#!/bin/bash
# Install or upgrade the exchange host (brier-db): PostgreSQL, the gateway, and the operator.
#   sudo bash install-db.sh /path/to/brier-release.tar.gz
# Safe to rerun: secrets are generated once and kept in /etc/brier (mode 600).
set -euo pipefail
RELEASE=${1:?usage: install-db.sh brier-release.tar.gz}

swap() {
  if ! swapon --show | grep -q /swapfile; then
    fallocate -l 2G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile
    grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
  fi
}
swap
export DEBIAN_FRONTEND=noninteractive
apt-get -o DPkg::Lock::Timeout=900 update -qq
apt-get -o DPkg::Lock::Timeout=900 install -y -qq postgresql python3-venv openssl >/dev/null

id brier >/dev/null 2>&1 || useradd --system --home /var/lib/brier --shell /usr/sbin/nologin brier
install -d -o brier -g brier -m 750 /var/lib/brier /var/lib/brier/journal /var/lib/brier/operator
install -d -m 755 /opt/brier
install -d -m 750 -g brier /etc/brier

# The release: binaries, the Python wheel and its hash-pinned requirements, these files.
STAGE=$(mktemp -d)
tar -xzf "$RELEASE" -C "$STAGE"
install -d /opt/brier/bin
install -m 755 "$STAGE"/bin/brier-gateway "$STAGE"/bin/brier-core /opt/brier/bin/
[ -d /opt/brier/venv ] || python3 -m venv /opt/brier/venv
/opt/brier/venv/bin/pip install -q --require-hashes -r "$STAGE"/py/requirements.txt
/opt/brier/venv/bin/pip install -q --no-deps --force-reinstall "$STAGE"/py/*.whl
cp "$STAGE"/COMMIT /opt/brier/COMMIT

# PostgreSQL: the brier role and database, sized for this server.
cp "$STAGE"/deploy/postgresql.conf /etc/postgresql/16/main/conf.d/brier.conf
systemctl restart postgresql
if [ ! -f /etc/brier/gateway.env ]; then
  DBPW=$(openssl rand -hex 24)
  sudo -u postgres psql -qc "CREATE ROLE brier LOGIN PASSWORD '$DBPW'"
  sudo -u postgres psql -qc "CREATE DATABASE brier OWNER brier"
  umask 077
  cat > /etc/brier/gateway.env <<ENV
BRIER_DATABASE_URL=postgres://brier:$DBPW@127.0.0.1:5432/brier
BRIER_TOTP_KEY=$(openssl rand -hex 32)
BRIER_PROXY_KEY=$(openssl rand -hex 32)
BRIER_DATA_DIR=/var/lib/brier/journal
BRIER_LISTEN=0.0.0.0:8080
ENV
  grep '^BRIER_DATABASE_URL=' /etc/brier/gateway.env > /etc/brier/operator.env
fi
chown root:brier /etc/brier/*.env && chmod 640 /etc/brier/*.env

install -m 644 "$STAGE"/deploy/systemd/brier-gateway.service "$STAGE"/deploy/systemd/brier-operator.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable -q brier-gateway brier-operator
systemctl restart brier-gateway
for _ in $(seq 1 30); do curl -sf http://127.0.0.1:8080/v1/health >/dev/null && break; sleep 1; done
curl -sf http://127.0.0.1:8080/v1/health
echo
systemctl restart brier-operator
# SSH: keys only, no root login, no X11 (deploy/sshd-hardening.conf). Checked before reloading,
# so a bad file never locks the operator out.
install -m 644 "$STAGE"/deploy/sshd-hardening.conf /etc/ssh/sshd_config.d/10-brier.conf
sshd -t && systemctl reload ssh
# System and security logs are kept 30 days (the privacy policy says so).
install -d -m 755 /etc/systemd/journald.conf.d
printf '[Journal]\nMaxRetentionSec=30day\n' > /etc/systemd/journald.conf.d/10-brier.conf
chmod 644 /etc/systemd/journald.conf.d/10-brier.conf
systemctl restart systemd-journald
# Nightly backups: to brier-web, and an encrypted copy to the private GitHub repository.
install -m 755 "$STAGE"/deploy/backup.sh /usr/local/bin/brier-backup
install -m 755 "$STAGE"/deploy/github-backup.sh /usr/local/bin/brier-github-backup
install -m 644 "$STAGE"/deploy/systemd/brier-backup.service "$STAGE"/deploy/systemd/brier-backup.timer /etc/systemd/system/
systemctl daemon-reload
systemctl enable -q --now brier-backup.timer
rm -rf "$STAGE"
echo "brier-db: $(cat /opt/brier/COMMIT)"
