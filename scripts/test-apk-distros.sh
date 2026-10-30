#!/bin/sh
# Build and exercise Caerus's APK backend inside Chimera and Alpine containers.
#
# Usage: scripts/test-apk-distros.sh [IMAGE | ssh:HOST ...]
#   default images: chimeralinux/chimera:latest alpine:latest alpine:edge
#   ssh:HOST  run on a disposable machine over SSH as root instead (e.g. a VM);
#             the source goes to /src there and packages ARE changed on it
#   SRC=dir   tree to test (default: this checkout)
#   OUT=dir   where logs/screenshots go (default: ./temp/apk-distros)
#   GUI=0     skip the Xvfb GUI run
#
# Each image gets a fresh root container (podman, host network because rootless
# sysfs mounts fail here). The checkout is mounted read-only and copied into the
# container, so nothing on the host changes. Steps: install build deps, build
# the apk variant, run unit tests, probe every `apk query` the backend issues,
# drive caerus-helper over its line protocol (installs/removes `tree` inside
# the container only) and optionally screenshot the GUI under Xvfb.
set -eu

here=${SRC:-$(cd "$(dirname "$0")/.." && pwd)}
out=${OUT:-$here/temp/apk-distros}
gui=${GUI:-1}
[ $# -gt 0 ] || set -- chimeralinux/chimera:latest alpine:latest alpine:edge
mkdir -p "$out"

inner='
set -u
step() { printf "\n=== %s\n" "$*"; }
ok() { printf "PASS %s\n" "$*"; }
bad() { printf "FAIL %s\n" "$*"; fails=$((fails + 1)); }
fails=0
. /etc/os-release; VERSION_ID=${VERSION_ID:-rolling}
step "system: $ID $VERSION_ID / $(apk --version)"
apk upgrade --interactive=no -q >/dev/null 2>&1 || apk upgrade -q || true
echo "after upgrade: $(apk --version)"
if [ "$ID" = chimera ]; then
    deps="rust cargo gtk4-devel glib-devel pkgconf polkit jq weston xwd imagemagick fonts-dejavu dbus"
else
    deps="rust cargo gtk4.0-dev glib-dev pkgconf polkit jq xvfb xwd imagemagick font-dejavu dbus build-base"
fi
step "deps: $deps"
for d in $deps; do apk add -q --interactive=no "$d" >/dev/null 2>&1 || echo "  (could not install $d)"; done
echo "rustc: $(rustc --version 2>&1)"

mkdir -p /build && for f in /src/* /src/.[!.]*; do case ${f##*/} in target|temp|.git) ;; *) cp -a "$f" /build/ ;; esac; done
cd /build
step build
if cargo build --release --locked -q -p caerus -p caerus-helper --no-default-features --features caerus/apk 2>&1 | tail -30; [ -x target/release/caerus ]; then ok build; else bad build; fi
step "unit tests (caerus apk)"
if cargo test -q --locked -p caerus --no-default-features --features apk >/tmp/t.log 2>&1; then ok "caerus tests"; else bad "caerus tests"; fi
grep -E "^test result|FAILED|panicked" /tmp/t.log
step "unit tests (helper)"
if cargo test -q --locked -p caerus-helper >/tmp/t.log 2>&1; then ok "helper tests"; else bad "helper tests"; fi
grep -E "^test result|FAILED|panicked" /tmp/t.log

step "apk queries used by the backend"
apk update -q >/dev/null 2>&1
F=name,version,description,arch,license,maintainer,url,file-size,installed-size,repositories
for q in "--from repositories" "--installed" "--upgradable"; do
    if apk query $q --fields $F --format json "*" >/tmp/q.json 2>/tmp/q.err && jq -e "type==\"array\" and length>0" /tmp/q.json >/dev/null; then
        ok "query $q: $(jq length /tmp/q.json) records, sample: $(jq -c ".[0]|{name,version,repositories}" /tmp/q.json)"
    elif [ "$q" = --upgradable ] && jq -e "type==\"array\"" /tmp/q.json >/dev/null 2>&1; then
        ok "query $q: 0 records"
    else
        bad "query $q: $(head -c 300 /tmp/q.err)"
    fi
done
echo "depends of jq: $(apk query --fields depends --format json jq 2>&1 | jq -c ".[0].depends" 2>&1 | head -c 200)"
for f in --rdepends --contents --provides --replaces; do
    n=$(apk info $f musl 2>/dev/null | grep -cv "^$")
    echo "info $f musl: $n lines"
done
step "simulation output (preview parser input)"
LC_ALL=C apk add --simulate --no-progress -- tree 2>&1 | head -5
LC_ALL=C apk del --simulate --no-progress -- jq 2>&1 | head -5
LC_ALL=C apk del --simulate --no-progress -- musl 2>&1 | head -5

step "helper protocol"
printf "%s\n" "SYNC" "INSTALL tree" "REMOVE tree" "PURGE tree" "HOLD tree" "ADDREPO https://example.invalid" "INSTALL --help" "UPGRADE" "QUIT" \
    | target/release/caerus-helper >/tmp/helper.log 2>&1
grep -v "^LOG" /tmp/helper.log | grep -v "^EVENT" ; echo "--- LOG lines: $(grep -c ^LOG /tmp/helper.log)"
grep -E "^LOG .*(xbps|not found|No such)" /tmp/helper.log | head -5
if apk info -e tree >/dev/null 2>&1; then bad "tree still installed after REMOVE"; fi
grep -q "^ERROR.*not supported by the APK backend" /tmp/helper.log && ok "void-only verbs refused" || bad "void-only verbs not refused (helper does not detect APK on $ID)"

step "install.sh round trip"
if ./install.sh >/tmp/inst.log 2>&1 && [ -x /usr/bin/caerus ] && [ -x /usr/libexec/caerus-helper ] \
    && [ -f /usr/share/polkit-1/actions/org.voidlinux.caerus.policy ]; then ok "install.sh"; else bad "install.sh: $(tail -3 /tmp/inst.log)"; fi
./install.sh --uninstall >/dev/null 2>&1 && [ ! -e /usr/bin/caerus ] && ok "install.sh --uninstall" || bad "install.sh --uninstall"

step "get-caerus.sh distro detection"
sed -n "/^os_id=/,/^as_root=/p" get-caerus.sh | sed "s/^as_root=.*/echo DETECTED=\$DISTRO/" >/tmp/detect.sh
sh /tmp/detect.sh 2>&1 | tail -2

if [ "$GUI" = 1 ]; then
    step "GUI smoke test"
    export GSK_RENDERER=cairo NO_AT_BRIDGE=1
    shot=/out/$ID-$VERSION_ID.png
    if command -v Xvfb >/dev/null; then
        Xvfb :99 -screen 0 1400x900x24 >/dev/null 2>&1 &
        export DISPLAY=:99 GDK_BACKEND=x11
        snap() { xwd -root -silent | convert xwd:- "$shot"; }
    elif command -v weston >/dev/null; then
        export XDG_RUNTIME_DIR=/tmp/xdg WAYLAND_DISPLAY=wl-caerus GDK_BACKEND=wayland
        mkdir -p -m 700 $XDG_RUNTIME_DIR
        weston --backend=headless --renderer=pixman --width=1400 --height=900 \
            --socket=$WAYLAND_DISPLAY --debug >/tmp/weston.log 2>&1 &
        snap() { (cd /tmp && weston-screenshooter >/dev/null 2>&1) && convert /tmp/wayland-screenshot-*.png "$shot"; }
    else
        snap() { false; }
    fi
    sleep 3
    if cargo test -q --locked -p caerus --no-default-features --features apk -- --ignored --test-threads=1 >/tmp/t.log 2>&1; then ok "GTK smoke test"; else bad "GTK smoke test"; fi
    grep -E "^test result|FAILED|panicked|confirmed by apk|needs .* by our graph" /tmp/t.log
    dbus-run-session -- target/release/caerus >/tmp/gui.log 2>&1 &
    gui_pid=$!
    sleep 25
    if kill -0 $gui_pid 2>/dev/null; then ok "GUI still running after 25s"; else bad "GUI exited early"; fi
    snap && ok "screenshot $shot" || bad screenshot
    kill $gui_pid 2>/dev/null
    echo "--- gui log (tail)"; tail -15 /tmp/gui.log
fi
printf "\nSUMMARY %s %s: %d failure(s)\n" "$ID" "$VERSION_ID" "$fails"
'

for img in "$@"; do
    tag=$(echo "$img" | tr '/:' '__')
    echo "##### $img -> $out/$tag.log"
    case $img in ssh:*)
        host=${img#ssh:}
        tar -C "$here" --exclude=./target --exclude=./.git --exclude=./temp -cf - . |
            ssh "$host" 'rm -rf /src /out && mkdir -p /src /out && tar -xf - -C /src'
        printf '%s' "$inner" | ssh "$host" "GUI=$gui sh -s" >"$out/$tag.log" 2>&1 || true
        scp -q "$host:/out/*.png" "$out/" 2>/dev/null || true
        grep -E '^(PASS|FAIL|SUMMARY)' "$out/$tag.log" || true
        continue
    esac
    podman run --rm --net=host -e GUI="$gui" \
        -v "$here:/src:ro" -v "$out:/out" \
        -v "caerus-cargo-$tag:/root/.cargo" \
        "$img" sh -c "$inner" >"$out/$tag.log" 2>&1 || true
    grep -E '^(PASS|FAIL|SUMMARY)' "$out/$tag.log" || true
done
