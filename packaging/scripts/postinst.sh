#!/bin/sh
set -e

getent group odin >/dev/null 2>&1 || groupadd --system odin
id -u odin >/dev/null 2>&1 || useradd --system --gid odin --no-create-home \
    --home-dir /var/lib/odin --shell /usr/sbin/nologin \
    --comment "Odin Valheim server manager" odin

install -d -m 0750 -o odin -g odin /var/lib/odin
# /etc/odin itself must stay world-traversable: Paths::resolve() detects
# system mode by stat()-ing config.toml, which any user must be able to do
# regardless of sudo/group membership. Only the file's own content is
# access-restricted below.
install -d -m 0755 -o root -g odin /etc/odin
if [ -f /etc/odin/config.toml ]; then
    chown root:odin /etc/odin/config.toml
    chmod 0640 /etc/odin/config.toml
fi

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload || true
    systemctl enable odin.service || true
    # By this point dpkg/rpm has already replaced the binary, but a
    # previously-running odin.service is still executing the old
    # (now-unlinked) one, so is-active still reflects its pre-upgrade
    # state. Restart only if it was already running, so a fresh install
    # still doesn't auto-start (see README's manual `systemctl start`).
    if systemctl is-active --quiet odin.service; then
        systemctl restart odin.service || true
    fi
fi

echo "Odin installed. The 'odin' system account owns /etc/odin and /var/lib/odin."
echo "Start the dashboard with:  sudo systemctl start odin.service"
echo "Run instance commands directly with:  sudo -u odin odin <command>"
