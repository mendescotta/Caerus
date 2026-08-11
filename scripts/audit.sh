#!/usr/bin/env bash
set -euo pipefail

# Generate a ripgrep JSON audit of risky patterns.

patterns=(
  'unwrap\('
  'expect\('
  'downcast\('
  'downcast_ref'
  'panic!'
  'unsafe'
  'TODO'
)

if ! command -v rg >/dev/null 2>&1; then
  echo "ripgrep (rg) is required. Install it or run audit inside CI where it's installed." >&2
  exit 0
fi

# Run ripgrep JSON output across the repo (excluding .git and target directories).
rg --json -n -S -g '!:target' -g '!.git' \
  -e "${patterns[0]}" -e "${patterns[1]}" -e "${patterns[2]}" -e "${patterns[3]}" -e "${patterns[4]}" -e "${patterns[5]}" -e "${patterns[6]}" \
  || true > audit-report.json

echo "Wrote audit-report.json (may be empty)."
