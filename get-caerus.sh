#!/bin/sh
set -e

REPO_URL="https://github.com/mendescotta/Caerus.git"
SRC_DIR="${CAERUS_SRC_DIR:-$PWD/Caerus}"
# A release tag by default; CAERUS_REF=main builds the development branch.
REF="${CAERUS_REF:-}"

TTY=/dev/tty

ask() {
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

os_id=""
if [ -r /etc/os-release ]; then
    os_id="$(. /etc/os-release && echo "${ID:-}")"
fi

if [ "$os_id" = chimera ] && command -v apk >/dev/null 2>&1; then
    DISTRO=chimera
    BUILD_PKGS="gtk4-devel glib-devel pkgconf polkit"
    has_pkg() { apk info -e "$1" >/dev/null 2>&1; }
    pkg_install="apk add --interactive=no"
    # The xbps backend links libxbps; Chimera builds the APK backend instead.
    build() {
        cargo build --release -p caerus -p caerus-helper \
            --no-default-features --features caerus/apk
    }
elif command -v xbps-install >/dev/null 2>&1; then
    DISTRO=void
    BUILD_PKGS="gtk4-devel libxbps-devel glib-devel clang pkg-config polkit"
    has_pkg() { xbps-query "$1" >/dev/null 2>&1; }
    pkg_install="xbps-install -Sy"
    build() { cargo build --release; }
else
    echo "Caerus supports Void Linux (XBPS) and Chimera Linux (APK);" >&2
    echo "neither 'xbps-install' nor a Chimera 'apk' was found." >&2
    exit 1
fi

as_root=""
if [ "$(id -u)" -ne 0 ]; then
    for tool in doas sudo; do
        if command -v "$tool" >/dev/null 2>&1; then
            as_root="$tool"
            break
        fi
    done
fi

missing_bins=""
for bin in git cargo rustc; do
    command -v "$bin" >/dev/null 2>&1 || missing_bins="$missing_bins $bin"
done
to_install=""
for pkg in $BUILD_PKGS; do
    has_pkg "$pkg" || to_install="$to_install $pkg"
done
case "$missing_bins" in *git*) to_install="$to_install git" ;; esac
case "$missing_bins" in *cargo*|*rustc*) to_install="$to_install rust cargo" ;; esac

if [ -n "$to_install" ]; then
    echo "Missing build dependencies:$to_install"
    if [ "$(id -u)" -ne 0 ] && [ -z "$as_root" ]; then
        echo "Install them as root ($pkg_install$to_install) and re-run." >&2
        exit 1
    fi
    if ask "Install them now via '${as_root:+$as_root }$pkg_install$to_install'?" y; then
        $as_root $pkg_install $to_install
    else
        echo "Can't build without them — install manually and re-run." >&2
        exit 1
    fi
fi

if [ -z "$REF" ]; then
    REF="$(git ls-remote --tags --refs --sort=-v:refname "$REPO_URL" 'v*' \
        | head -n 1 | sed 's|.*refs/tags/||')"
    REF="${REF:-main}"
fi
echo "Building Caerus $REF for $DISTRO."

if [ -d "$SRC_DIR/.git" ]; then
    echo "Found an existing checkout at $SRC_DIR — updating it to $REF."
    git -C "$SRC_DIR" fetch --depth 1 origin "$REF"
    git -C "$SRC_DIR" checkout -q --detach FETCH_HEAD
elif [ -e "$SRC_DIR" ]; then
    echo "$SRC_DIR exists and isn't a git checkout — remove it or set" >&2
    echo "CAERUS_SRC_DIR to somewhere else, then re-run." >&2
    exit 1
else
    git clone --depth 1 --branch "$REF" "$REPO_URL" "$SRC_DIR"
fi

cd "$SRC_DIR"
build

echo
echo "Built. What now?"
echo "  r) Run it without installing (straight out of this build tree)"
echo "  u) Register for this user only — real icon/name in Alt-Tab etc.,"
echo "     still runs from this build tree, no root (./install.sh --user)"
echo "  i) Install system-wide (./install.sh as root)"
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
        $as_root ./install.sh
        ;;
    *)
        echo "Built at $SRC_DIR/target/release/caerus."
        echo "Run it directly, './install.sh --user' to register it for this"
        echo "user (no root), or run './install.sh' as root to install"
        echo "system-wide — all from $SRC_DIR when ready."
        ;;
esac
