#!/usr/bin/env bash
# Build Fastshot and install it for the current user (~/.local).
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
CARGO="${CARGO:-$(command -v cargo || echo "$HOME/.cargo/bin/cargo")}"

"$CARGO" install --path . --root "$PREFIX" --force
APP_ID=io.github.Pager_290583.Fastshot
install -Dm644 "assets/$APP_ID.svg" "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "assets/$APP_ID.desktop" "$PREFIX/share/applications/$APP_ID.desktop"
sed -i "s|^Exec=fastshot|Exec=$PREFIX/bin/fastshot|" "$PREFIX/share/applications/$APP_ID.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" || true

echo "Installed to $PREFIX/bin/fastshot"
if [[ "${1:-}" == "--bind-print" ]]; then
    "$PREFIX/bin/fastshot" install-shortcut --key Print
fi
echo "Tip: bind the Print key with:  fastshot install-shortcut"
