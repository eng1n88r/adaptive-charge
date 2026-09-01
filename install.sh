#!/usr/bin/env bash
# adaptive-charge installer:
#   curl -fsSL https://raw.githubusercontent.com/eng1n88r/adaptive-charge/master/install.sh | bash
#
# On Arch-based systems, builds and installs the pacman package from the
# prebuilt release (pacman tracks every file). Elsewhere, installs the release
# tarball directly, or builds from source with cargo as a last resort.
# Finishes by enabling the daemon and seeding it from UPower's history.
set -euo pipefail

REPO=eng1n88r/adaptive-charge

[ "$(uname -s)" = Linux ] || { echo "error: Linux only" >&2; exit 1; }

if ! ls /sys/class/power_supply/BAT*/charge_control_end_threshold >/dev/null 2>&1; then
    if [ "${ADAPTIVE_CHARGE_FORCE:-0}" != 1 ]; then
        echo "error: no battery here exposes charge_control_end_threshold -" >&2
        echo "       this hardware cannot cap charging, so the daemon would never start." >&2
        echo "       (set ADAPTIVE_CHARGE_FORCE=1 to install anyway)" >&2
        exit 1
    fi
    echo "warning: no charge threshold support detected, installing anyway (forced)"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

installed=""
if command -v pacman >/dev/null 2>&1 && command -v makepkg >/dev/null 2>&1; then
    echo "==> Arch detected: installing as a pacman package"
    git clone --quiet --depth 1 "https://github.com/$REPO" "$tmp/src"
    (cd "$tmp/src/packaging/bin" && makepkg -si --noconfirm)
    installed=pacman
elif [ "$(uname -m)" = x86_64 ]; then
    ver=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep -om1 '"tag_name": *"v[^"]*"' | grep -o 'v[0-9.]*')
    url="https://github.com/$REPO/releases/download/$ver/adaptive-charge-${ver#v}-x86_64.tar.gz"
    echo "==> downloading $url"
    if curl -fsSL "$url" -o "$tmp/ac.tar.gz"; then
        tar -xzf "$tmp/ac.tar.gz" -C "$tmp"
        # tarball paths target /usr/bin (for packaging); direct installs
        # belong in /usr/local, so repoint the unit and sudoers entries
        sed -i 's|/usr/bin/adaptive-charge|/usr/local/bin/adaptive-charge|g' \
            "$tmp/adaptive-charge.service" "$tmp/sudoers-adaptive-charge"
        sudo install -Dm755 "$tmp/adaptive-charge" /usr/local/bin/adaptive-charge
        sudo install -Dm644 "$tmp/adaptive-charge.service" /etc/systemd/system/adaptive-charge.service
        sudo install -Dm440 "$tmp/sudoers-adaptive-charge" /etc/sudoers.d/adaptive-charge
        sudo systemctl daemon-reload
        installed=tarball
    else
        echo "download failed; falling back to source build"
    fi
fi

if [ -z "$installed" ]; then
    for dep in git cargo make; do
        command -v "$dep" >/dev/null 2>&1 || {
            echo "error: no package/prebuilt path available and '$dep' is missing" >&2
            exit 1
        }
    done
    git clone --quiet --depth 1 "https://github.com/$REPO" "$tmp/src"
    (cd "$tmp/src" && make build && sudo make install)
    installed=source
    echo "==> installed from source ($installed path handles enable+seed itself)"
    exit 0
fi

echo "==> enabling the daemon"
sudo systemctl enable --now adaptive-charge.service
echo "==> seeding from UPower history (best effort)"
sudo adaptive-charge seed --write || true
echo
adaptive-charge status
echo
echo "installed via: $installed"
echo "Omarchy users: add the panel widget with"
echo "  omarchy plugin add https://github.com/eng1n88r/omarchy-adaptive-power.git --enable"
