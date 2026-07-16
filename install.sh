#!/bin/sh
# Installs, registers, or removes caerus + caerus-helper + data files.
# Two independent switches:
#
#   ./install.sh                 system-wide, needs root — installs into
#                                 ${PREFIX:-/usr}: binary, helper, polkit
#                                 policy, .desktop entry, metainfo, icons.
#                                 Run `cargo build --release` first.
#
#   ./install.sh --user          this user only, no root — registers a
#                                 .desktop entry + icon under
#                                 ~/.local/share pointing at whichever
#                                 build (release preferred, else debug)
#                                 exists in this checkout, so the desktop
#                                 shell (Alt-Tab, Overview, top bar) shows
#                                 the real name/icon for an uninstalled
#                                 build. No polkit policy or metainfo —
#                                 those only make sense system-wide.
#
#   ./install.sh --uninstall     reverses a system-wide install.
#   ./install.sh --user --uninstall
#                                 reverses a --user registration.
#
# Safe to run --uninstall even if nothing was ever installed (every
# removal is a plain `rm -f`, not an error if the file's already gone).
set -e

MODE=install
SCOPE=system
for arg in "$@"; do
    case "$arg" in
        --uninstall) MODE=uninstall ;;
        --user) SCOPE=user ;;
        *)
            echo "Usage: $0 [--user] [--uninstall]" >&2
            exit 1
            ;;
    esac
done

SRC_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ "$SCOPE" = system ]; then
    if [ "$(id -u)" -ne 0 ]; then
        echo "System-wide mode needs root, e.g.:" >&2
        echo "  sudo $0${MODE:+ --uninstall}" >&2
        echo "...or add --user for a no-root, this-user-only install." >&2
        exit 1
    fi
    PREFIX="${PREFIX:-/usr}"
    BINDIR="$PREFIX/bin"
    LIBEXECDIR="$PREFIX/libexec"
    DATADIR="$PREFIX/share"
else
    if [ "$(id -u)" -eq 0 ]; then
        echo "--user mode shouldn't run as root — it writes into the" >&2
        echo "invoking user's own home directory. Drop --user to install" >&2
        echo "system-wide instead." >&2
        exit 1
    fi
    DATADIR="${XDG_DATA_HOME:-$HOME/.local/share}"
fi

DESKTOP_FILE="$DATADIR/applications/org.voidlinux.caerus.desktop"
APP_ICON="$DATADIR/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg"

# Every bundled symbolic-icon fallback path this app installs, relative
# to hicolor/ — one source of truth shared between install and
# uninstall (and both scopes) so they can never drift out of sync with
# each other. See `ensure_icon_theme_fallback` in caerus/src/ui/window.rs
# for why these are bundled at all (not every desktop's active icon
# theme is guaranteed to have them).
symbolic_icon_paths() {
    find "$SRC_DIR/caerus/data/icons/hicolor/symbolic" -name '*.svg' | while read -r svg; do
        echo "${svg#"$SRC_DIR/caerus/data/icons/"}"
    done
}

refresh_desktop_caches() {
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$DATADIR/icons/hicolor" >/dev/null 2>&1 || true
    fi
    if [ "$SCOPE" = user ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$DATADIR/applications" >/dev/null 2>&1 || true
    fi
}

if [ "$MODE" = uninstall ]; then
    if [ "$SCOPE" = system ]; then
        rm -fv \
            "$BINDIR/caerus" \
            "$LIBEXECDIR/caerus-helper" \
            "$DATADIR/polkit-1/actions/org.voidlinux.caerus.policy" \
            "$DATADIR/metainfo/org.voidlinux.caerus.metainfo.xml" \
            "$DESKTOP_FILE" \
            "$APP_ICON"
    else
        rm -fv "$DESKTOP_FILE" "$APP_ICON"
    fi
    symbolic_icon_paths | while read -r rel; do
        rm -fv "$DATADIR/icons/$rel"
    done
    refresh_desktop_caches
    echo "Uninstalled ($SCOPE)."
    exit 0
fi

# --- install ---

if [ "$SCOPE" = system ]; then
    TARGET_DIR="$SRC_DIR/target/release"
    if [ ! -x "$TARGET_DIR/caerus" ] || [ ! -x "$TARGET_DIR/caerus-helper" ]; then
        echo "Build first: cargo build --release" >&2
        exit 1
    fi

    install -Dm755 "$TARGET_DIR/caerus" "$BINDIR/caerus"
    install -Dm755 "$TARGET_DIR/caerus-helper" "$LIBEXECDIR/caerus-helper"
    install -Dm644 "$SRC_DIR/caerus/data/org.voidlinux.caerus.policy" \
        "$DATADIR/polkit-1/actions/org.voidlinux.caerus.policy"
    install -Dm644 "$SRC_DIR/caerus/data/org.voidlinux.caerus.desktop" "$DESKTOP_FILE"
    install -Dm644 "$SRC_DIR/caerus/data/org.voidlinux.caerus.metainfo.xml" \
        "$DATADIR/metainfo/org.voidlinux.caerus.metainfo.xml"
    install -Dm644 "$SRC_DIR/caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" \
        "$APP_ICON"
    symbolic_icon_paths | while read -r rel; do
        install -Dm644 "$SRC_DIR/caerus/data/icons/$rel" "$DATADIR/icons/$rel"
    done

    refresh_desktop_caches
    echo "Installed. Launch with: caerus"
else
    if [ -x "$SRC_DIR/target/release/caerus" ]; then
        BIN="$SRC_DIR/target/release/caerus"
    elif [ -x "$SRC_DIR/target/debug/caerus" ]; then
        BIN="$SRC_DIR/target/debug/caerus"
    else
        echo "Build first: cargo build (or cargo build --release)" >&2
        exit 1
    fi

    mkdir -p "$(dirname "$DESKTOP_FILE")" "$(dirname "$APP_ICON")"
    sed "s|^Exec=caerus\$|Exec=$BIN|" "$SRC_DIR/caerus/data/org.voidlinux.caerus.desktop" \
        > "$DESKTOP_FILE"
    # Icons are byte-identical to the repo copies, so symlink rather than
    # duplicate them — and re-running this after a repo update picks up
    # new/changed icons for free.
    ln -sf "$SRC_DIR/caerus/data/icons/hicolor/scalable/apps/org.voidlinux.caerus.svg" "$APP_ICON"
    symbolic_icon_paths | while read -r rel; do
        mkdir -p "$DATADIR/icons/$(dirname "$rel")"
        ln -sf "$SRC_DIR/caerus/data/icons/$rel" "$DATADIR/icons/$rel"
    done

    refresh_desktop_caches
    echo "Registered $BIN for this user."
    echo "(Re)launch caerus — the desktop shell will now show its real icon and name."
fi
