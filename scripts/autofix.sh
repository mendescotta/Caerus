#!/usr/bin/env bash
set -euo pipefail

# Generate an audit report and a small suggested-fixes markdown file.
# This script intentionally does NOT apply automated code edits. It
# prepares artifacts that the workflow will commit into a draft PR for
# manual review and surgical edits.

ts=$(date -u +%Y%m%dT%H%M%SZ)
outdir=".github/auto-fixes"
mkdir -p "$outdir"

./scripts/audit.sh

suggested="$outdir/${ts}-suggested-fixes.md"
{
  echo "# Suggested fixes ($ts)"
  echo
  echo "This file lists the grep matches the audit found. Review each
match and apply small, surgical edits (avoid broad automated refactors)."
  echo
  echo '```'
  rg -n --no-heading -S -g '!:target' -g '!.git' \
    -e 'unwrap\(' -e 'expect\(' -e 'downcast\(' -e 'downcast_ref' -e 'panic!' -e 'unsafe' -e 'TODO' || true
  echo '```'
} > "$suggested"

# Stage the audit report and suggested-fixes for the PR creator action to commit.
# The create-pull-request action will create a branch and commit these files.

echo "Prepared $suggested and audit-report.json for PR creation."
