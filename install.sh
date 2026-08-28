#!/bin/sh
# Downloads the latest odin-server .deb/.rpm from GitHub Releases and
# installs it with the host's package manager.
#
#   curl -sSL https://raw.githubusercontent.com/Blad3Mak3r/odin/main/install.sh | sh
#
# Only Debian-family (apt) and Fedora/RHEL-family (dnf) hosts on x86_64 are
# supported, matching what the release workflow builds and publishes.

set -eu

REPO="Blad3Mak3r/odin"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

log() {
    printf '==> %s\n' "$1"
}

die() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$1"
    else
        die "neither curl nor wget is available; install one and re-run."
    fi
}

download_to() {
    dest="$1"
    url="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$dest" "$url"
    else
        wget -qO "$dest" "$url"
    fi
}

arch="$(uname -m)"
if [ "$arch" != "x86_64" ]; then
    die "unsupported architecture '$arch'; odin only ships x86_64 builds. Build from source instead (see README)."
fi

if [ -f /etc/debian_version ]; then
    family="debian"
    ext="deb"
elif [ -f /etc/redhat-release ] || [ -f /etc/fedora-release ]; then
    family="redhat"
    ext="rpm"
else
    die "unsupported distro (no /etc/debian_version, /etc/redhat-release, or /etc/fedora-release found). Build from source instead (see README: 'make install' / 'make install-user')."
fi

if [ "$(id -u)" -eq 0 ]; then
    sudo=""
elif command -v sudo >/dev/null 2>&1; then
    sudo="sudo"
else
    die "this script needs root privileges to install a system package, and 'sudo' was not found. Re-run as root."
fi

log "fetching latest release metadata..."
release_json="$(fetch "$API_URL")" || die "failed to reach GitHub's API ($API_URL)."

asset_url="$(printf '%s\n' "$release_json" \
    | grep -o '"browser_download_url": *"[^"]*"' \
    | grep "\.${ext}\"$" \
    | head -n1 \
    | sed -E 's/^"browser_download_url": *"([^"]+)"$/\1/')"

[ -n "$asset_url" ] || die "couldn't find a .$ext asset in the latest release. Check https://github.com/${REPO}/releases/latest manually."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

pkg_file="$tmpdir/$(basename "$asset_url")"
log "downloading $(basename "$asset_url")..."
download_to "$pkg_file" "$asset_url"

log "installing via $([ "$family" = "debian" ] && echo apt || echo dnf) (needs sudo)..."
if [ "$family" = "debian" ]; then
    $sudo apt-get install -y "$pkg_file"
else
    if command -v dnf >/dev/null 2>&1; then
        $sudo dnf install -y "$pkg_file"
    else
        $sudo rpm -Uvh "$pkg_file"
    fi
fi

log "done. Run 'odin doctor' to verify the install, then 'sudo systemctl enable --now odin.service' to start the dashboard."
