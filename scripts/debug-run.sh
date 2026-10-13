#!/bin/bash
# Debug launcher for daily driving — builds the 'daily' profile, logs all
# output to a timestamped file, and captures core dumps to the same directory.
#
# Usage:
#   ./scripts/debug-run.sh [--fatal-criticals] [--no-build]
#
#   --fatal-criticals  abort on the first GLib/GTK g_critical() instead of
#                      letting the process limp to a later null-deref; gives a
#                      clean backtrace at the point of the bad GTK state.
#                      Useful when reproducing the remove-confirm segfault.
#   --no-build         skip 'cargo build' (use an existing binary)

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
LOGDIR="${XDG_DATA_HOME:-$HOME/.local/share}/caerus/debug"
mkdir -p "$LOGDIR"

FATAL_CRITICALS=0
DO_BUILD=1
for arg in "$@"; do
    case "$arg" in
        --fatal-criticals) FATAL_CRITICALS=1 ;;
        --no-build)        DO_BUILD=0 ;;
    esac
done

BIN="$REPO/target/daily/caerus"

if [[ $DO_BUILD -eq 1 ]]; then
    echo "Building (profile: daily)…"
    cargo build --manifest-path "$REPO/Cargo.toml" --profile daily --quiet
fi

if [[ ! -x "$BIN" ]]; then
    echo "Binary not found: $BIN" >&2
    echo "Run without --no-build to build first." >&2
    exit 1
fi

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
LOGFILE="$LOGDIR/caerus-$TIMESTAMP.log"

export RUST_BACKTRACE=full
export G_MESSAGES_DEBUG=all
[[ $FATAL_CRITICALS -eq 1 ]] && export G_DEBUG=fatal-criticals

ulimit -c unlimited

{
    echo "=== caerus debug run $TIMESTAMP ==="
    echo "binary : $BIN"
    echo "fatal-criticals: $FATAL_CRITICALS"
    echo "log    : $LOGFILE"
    echo "==="
} | tee "$LOGFILE"
echo ""

# Change to $LOGDIR so core dumps (pattern: 'core.<pid>') land there.
cd "$LOGDIR"
# set +e/-e brackets just this line: PIPESTATUS must be read immediately
# after the pipeline, before any other command (even '|| true') overwrites
# it, so errexit has to be off here rather than papered over with '|| true'.
set +e
"$BIN" 2>&1 | tee -a "$LOGFILE"
EXIT=${PIPESTATUS[0]}
set -e

# The reporting below must not mask $EXIT: a failed tee/printf (e.g. disk
# full) would otherwise abort the script under errexit before `exit "$EXIT"`
# runs, handing the caller the reporting command's exit code instead of
# Caerus's real one.
{
    echo ""
    echo "=== exit $EXIT ==="
} | tee -a "$LOGFILE" || true

# Report any fresh core dump.
CORE=$(ls -t "$LOGDIR"/core.* 2>/dev/null | head -1 || true)
if [[ -n "$CORE" ]]; then
    echo ""
    echo "Core dump: $CORE"
    echo "Backtrace: gdb $BIN $CORE"
    echo "           (gdb) bt full"
    printf "Core dump: %s\ngdb %s %s\n" "$CORE" "$BIN" "$CORE" >> "$LOGFILE" || true
fi

echo ""
echo "Log: $LOGFILE"
exit "$EXIT"
