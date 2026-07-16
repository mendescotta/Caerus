#!/bin/sh
# One-line installer: clones Caerus, builds it, then offers to run it
# locally or install it system-wide. Meant to be run as:
#
#   curl -fsSL https://raw.githubusercontent.com/mendescotta/Caerus/main/get-caerus.sh | sh
#
# This only ever builds from source on your own machine — there's no
# prebuilt binary here to trust, just this script (read it before piping
# it to sh, same as you should for any curl|sh installer) plus whatever
# `cargo build` itself pulls from crates.io.
set -e

REPO_URL="https://github.com/mendescotta/Caerus.git"
SRC_DIR="${CAERUS_SRC_DIR:-$PWD/Caerus}"

# curl | sh consumes stdin for the script itself, so interactive prompts
# below read from the controlling terminal directly instead.
TTY=/dev/tty

ask() {
    # ask "question" "default(y/n)" -> 0 for yes, 1 for no. Falls back to
    # the default with no prompt at all if there's no usable terminal
    # (e.g. run from a non-interactive shell) rather than hanging or
    # aborting on a tty open failure.
    prompt="$1"
    default="$2"
    reply=""
    if [ "$default" = "y" ]; then suffix="[Y/n]"; else suffix="[y/N]"; fi
    if { printf '%s %s ' "$prompt" "$suffix" > "$TTY"; } 2>/dev/null; then
        read -r reply < "$TTY" 2>/dev/null || reply=""
    fi
    case "$reply" in
        "") [ "$default" = "y" ] ;;
        [Yy]*) return 0 ;;
        *) return 1 ;;
    esac
}

if ! command -v xbps-install >/dev/null 2>&1; then
    echo "Caerus targets Void Linux — 'xbps-install' wasn't found, so this" >&2
    echo "script won't try to install build dependencies. See README.md's" >&2
    echo "'Dependencies' section if you're building on another distro." >&2
    exit 1
fi

# Binary-providing deps (checked via `command -v`) vs devel/library
# packages (checked via `xbps-query <pkgname>`'s exit code — an exact,
# unambiguous installed-package check, unlike grepping `xbps-query -l`
# output for a name prefix, which can false-positive on an installed
# package that merely starts with the same prefix, e.g. `clang-analyzer18`
# satisfying a naive check for plain `clang`) — same package list as
# README's "On Void Linux" line.
missing_bins=""
for bin in git cargo rustc; do
    command -v "$bin" >/dev/null 2>&1 || missing_bins="$missing_bins $bin"
done
missing_pkgs=""
for pkg in gtk4-devel libxbps-devel glib-devel clang pkg-config; do
    xbps-query "$pkg" >/dev/null 2>&1 || missing_pkgs="$missing_pkgs $pkg"
done
# git/cargo/rustc map to real xbps package names for the install command
# below (rustc/cargo both come from the single "rust"+"cargo" packages).
to_install=""
[ -n "$missing_pkgs" ] && to_install="$to_install$missing_pkgs"
case "$missing_bins" in *git*) to_install="$to_install git" ;; esac
case "$missing_bins" in *cargo*|*rustc*) to_install="$to_install rust cargo" ;; esac

if [ -n "$to_install" ]; then
    echo "Missing build dependencies:$to_install"
    if ask "Install them now via 'sudo xbps-install -y$to_install'?" y; then
        # Deliberately unquoted: $to_install is always a space-joined
        # list of this script's own literal package-name constants above,
        # never external input, so word-splitting it here is the intent,
        # not an injection risk.
        sudo xbps-install -Sy $to_install
    else
        echo "Can't build without them — install manually and re-run." >&2
        exit 1
    fi
fi

if [ -d "$SRC_DIR/.git" ]; then
    echo "Found an existing checkout at $SRC_DIR — pulling latest instead of re-cloning."
    git -C "$SRC_DIR" pull --ff-only
elif [ -e "$SRC_DIR" ]; then
    echo "$SRC_DIR exists and isn't a git checkout — remove it or set" >&2
    echo "CAERUS_SRC_DIR to somewhere else, then re-run." >&2
    exit 1
else
    git clone --depth 1 "$REPO_URL" "$SRC_DIR"
fi

cd "$SRC_DIR"
cargo build --release

echo
echo "Built. What now?"
echo "  r) Run it without installing (straight out of this build tree)"
echo "  u) Register for this user only — real icon/name in Alt-Tab etc.,"
echo "     still runs from this build tree, no root (./install.sh --user)"
echo "  i) Install system-wide (sudo ./install.sh)"
echo "  q) Nothing — I'll do it myself"
choice=q
if { printf 'Choice [r/u/i/q]: ' > "$TTY"; } 2>/dev/null; then
    read -r choice < "$TTY" 2>/dev/null || choice=q
fi

case "$choice" in
    [Rr]*)
        exec ./target/release/caerus
        ;;
    [Uu]*)
        ./install.sh --user
        exec ./target/release/caerus
        ;;
    [Ii]*)
        sudo ./install.sh
        ;;
    *)
        echo "Built at $SRC_DIR/target/release/caerus."
        echo "Run it directly, './install.sh --user' to register it for this"
        echo "user (no root), or 'sudo ./install.sh' to install system-wide —"
        echo "all from $SRC_DIR when ready."
        ;;
esac
