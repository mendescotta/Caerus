#!/bin/sh
# Installs caerus + caerus-helper + data files, mirroring the layout the
# original meson.build produced. Run `cargo build --release` first (see
# README.md), then this script as root (it's what does the actual
# privileged file copying + policy/icon/desktop registration — nothing
# in `cargo build` itself needs root).
set -e

PREFIX="${PREFIX:-/usr}"
BINDIR="$PREFIX/bin"
LIBEXECDIR="$PREFIX/libexec"
DATADIR="$PREFIX/share"

if [ "$(id -u)" -ne 0 ]; then
    echo "Run as root (e.g. sudo ./install.sh) — installs into $PREFIX" >&2
    exit 1
fi

SRC_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET_DIR="$SRC_DIR/target/release"

if [ ! -x "$TARGET_DIR/caerus" ] || [ ! -x "$TARGET_DIR/caerus-helper" ]; then
    echo "Build first: cargo build --release" >&2
    exit 1
fi

install -Dm755 "$TARGET_DIR/caerus"        "$BINDIR/caerus"
install -Dm755 "$TARGET_DIR/caerus-helper" "$LIBEXECDIR/caerus-helper"

install -Dm644 "$SRC_DIR/caerus/data/org.voidlinux.caerus.policy" \
    "$DATADIR/polkit-1/actions/org.voidlinux.caerus.policy"
install -Dm644 "$SRC_DIR/caerus/data/org.voidlinux.caerus.desktop" \
    "$DATADIR/applications/org.voidlinux.caerus.desktop"
install -Dm644 "$SRC_DIR/caerus/data/icons/scalable/apps/org.voidlinux.caerus.svg" \
    "$DATADIR/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$DATADIR/icons/hicolor" || true
fi

echo "Installed. Launch with: caerus"
