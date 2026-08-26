#!/bin/sh
set -e

if [ -d /run/systemd/system ]; then
    systemctl stop odin.service >/dev/null 2>&1 || true
    systemctl disable odin.service >/dev/null 2>&1 || true
    systemctl daemon-reload || true
fi

echo "Odin removed. /var/lib/odin (world saves, backups, mods) and the 'odin'"
echo "system account were left in place. To remove them entirely, run:"
echo "  sudo rm -rf /var/lib/odin /etc/odin"
echo "  sudo userdel odin && sudo groupdel odin"
