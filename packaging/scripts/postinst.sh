#!/bin/sh
set -e

install -d -m 0750 -o root -g root /var/lib/odin
# /etc/odin itself must stay world-traversable: Paths::resolve() detects
# system mode by stat()-ing config.toml, which any user must be able to do
# regardless of sudo/group membership. Only the file's own content is
# access-restricted below.
install -d -m 0755 -o root -g root /etc/odin
if [ -f /etc/odin/config.toml ]; then
    chown root:root /etc/odin/config.toml
    chmod 0600 /etc/odin/config.toml
fi

if [ -d /run/systemd/system ]; then
    # /run/odin (the supervisor.rs Unix control/events sockets live there —
    # see Paths::runtime_dir) is provisioned by the tmpfiles.d config below
    # rather than created on demand, preserving the expected permissions
    # before the root service starts its first instance.
    # --create applies it now, in addition to the usual on-boot pass, so a
    # fresh install/upgrade doesn't need a reboot before instances can start.
    systemd-tmpfiles --create /usr/lib/tmpfiles.d/odin.conf || true
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

echo "Odin installed. Root owns /etc/odin and /var/lib/odin."
echo "Start the dashboard with:  sudo systemctl start odin.service"
echo "Run instance commands directly with:  sudo odin <command>"
