#!/bin/sh
set -e

# This script is shared between dpkg's postrm and rpm's %postun, both of
# which also fire mid-*upgrade* (dpkg passes "upgrade"/"failed-upgrade"/
# "abort-*"; rpm passes a nonzero count of remaining installed versions),
# not only on a genuine final removal. Only tear the service down when
# this really is the last copy going away, or an upgrade would leave the
# service stopped and disabled right after dpkg/rpm hands off to the new
# package's postinst.
case "$1" in
    remove | purge)
        is_final_removal=true
        ;;
    upgrade | failed-upgrade | abort-install | abort-upgrade | disappear)
        is_final_removal=false
        ;;
    *)
        if [ "$1" = "0" ]; then
            is_final_removal=true
        else
            is_final_removal=false
        fi
        ;;
esac

if [ "$is_final_removal" = "true" ]; then
    if [ -d /run/systemd/system ]; then
        systemctl stop odin.service >/dev/null 2>&1 || true
        systemctl disable odin.service >/dev/null 2>&1 || true
        systemctl daemon-reload || true
    fi

    echo "Odin removed. /var/lib/odin (world saves, backups, mods) and the 'odin'"
    echo "system account were left in place. To remove them entirely, run:"
    echo "  sudo rm -rf /var/lib/odin /etc/odin"
    echo "  sudo userdel odin && sudo groupdel odin"
fi
