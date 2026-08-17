#!/usr/bin/env bash
# Give the Linux desktop what it needs to show the VP logo for the installer's window:
# the icon in the hicolor theme, and a desktop entry whose name matches the window's app_id.
# Wayland has no way for the program itself to set its icon, so this is the only route there.
#
#   packaging/install-desktop-entry.sh [path-to-civ5vp-installer]
#
# Everything lands under ~/.local/share — no root, nothing outside the user's own account.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$repo/target/release/civ5vp-installer}"
binary="$(readlink -f "$binary")"
[ -x "$binary" ] || { echo "not an executable: $binary" >&2; exit 1; }

logo="$repo/assets/icon/VP_logo.png"
share="${XDG_DATA_HOME:-$HOME/.local/share}"

for size in 16 24 32 48 64 128 256; do
    dir="$share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$dir"
    if command -v magick >/dev/null 2>&1; then
        magick "$logo" -background none -resize "${size}x${size}" "$dir/civ5vp-installer.png"
    else
        # No ImageMagick: one unscaled copy still beats a generic placeholder.
        cp "$logo" "$dir/civ5vp-installer.png"
    fi
done

mkdir -p "$share/applications"
sed "s|^Exec=.*|Exec=$binary|" "$repo/packaging/civ5vp-installer.desktop" \
    > "$share/applications/civ5vp-installer.desktop"

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$share/icons/hicolor" || true

echo "Installed the desktop entry and icons for: $binary"
