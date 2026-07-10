#!/bin/sh
# Registers caerus with the desktop shell (icon, proper app name, Alt-Tab/
# Overview entry) for testing straight out of the build tree. Unlike
# install.sh, this needs no root: it only writes into the current user's
# XDG data dirs (~/.local/share), pointing Exec at this checkout's binary.
#
# Without this, caerus still runs fine and its own UI (headerbar, About
# dialog) renders its icon correctly — but the desktop shell has no
# installed .desktop entry to match the running window against, so it
# falls back to a generic icon and the raw WM_CLASS ("caerus") instead of
# "Caerus" in places like the GNOME top bar, Alt-Tab, and the Overview.
set -e

SRC_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ -x "$SRC_DIR/target/release/caerus" ]; then
    BIN="$SRC_DIR/target/release/caerus"
elif [ -x "$SRC_DIR/target/debug/caerus" ]; then
    BIN="$SRC_DIR/target/debug/caerus"
else
    echo "Build first: cargo build (or cargo build --release)" >&2
    exit 1
fi

DATADIR="${XDG_DATA_HOME:-$HOME/.local/share}"

mkdir -p "$DATADIR/applications" "$DATADIR/icons/hicolor/scalable/apps"

sed "s|^Exec=caerus\$|Exec=$BIN|" "$SRC_DIR/caerus/data/org.voidlinux.caerus.desktop" \
    > "$DATADIR/applications/org.voidlinux.caerus.desktop"

ln -sf "$SRC_DIR/caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" \
    "$DATADIR/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$DATADIR/icons/hicolor" >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DATADIR/applications" >/dev/null 2>&1 || true
fi

echo "Registered $BIN for this user."
echo "(Re)launch caerus — the desktop shell will now show its real icon and name."
