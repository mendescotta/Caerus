#!/usr/bin/env bash
set -euo pipefail

# Generate a review-only audit report and suggested-fix notes.
# This is the conservative Option 2 workflow for Caerus: audit first,
# review the findings, and patch manually in small, reviewable commits.

readonly ts=$(date -u +%Y%m%dT%H%M%SZ)
readonly outdir=".github/auto-fixes"
mkdir -p "$outdir"

./scripts/audit.sh

readonly suggested="$outdir/${ts}-suggested-fixes.md"
{
echo "# Suggested fixes ($ts)"
echo
echo "This review artifact is intentionally not a code rewrite."
echo "Use it to guide a small, human-reviewed patch in one or two files at a time."
echo
echo "## Audit findings"
echo
echo '```'
rg -n --no-heading -S -g '!target/' -g '!.git/' \
  -e 'unwrap\(' -e 'expect\(' -e 'downcast\(' -e 'downcast_ref' -e 'panic!' -e 'unsafe' -e 'TODO' || true
echo '```'
} > "$suggested"

echo "Prepared $suggested and audit-report.json for manual review."
echo "No code was rewritten automatically."
