#!/usr/bin/env bash
# Install shadcn/ui components into src/shell/ui/.
#
# This script is the canonical way to (re)provision shadcn components.
# Run from web/ directory:  bash scripts/install-shadcn.sh
#
# The components are owned source — once installed, they live in this repo
# and are edited directly. Re-running adds missing ones; pass --overwrite
# to refresh existing files (caution: discards local edits).

set -euo pipefail

cd "$(dirname "$0")/.."

OVERWRITE=""
if [[ "${1:-}" == "--overwrite" ]]; then
  OVERWRITE="--overwrite"
fi

COMPONENTS=(
  # Originally installed (avatar/badge/button/...)
  button
  dialog
  popover
  tooltip
  dropdown-menu
  context-menu
  scroll-area
  tabs
  separator
  sonner
  command
  input
  badge
  switch
  select
  avatar
  sheet

  # Phase 6 M0.3 — data-display batch
  table
  pagination
  hover-card
  skeleton

  # Phase 6 M0.3 — form batch
  card
  form
  checkbox
  radio-group
  slider
  textarea
  label

  # Phase 6 M0.3 — feedback/navigation batch
  alert
  breadcrumb
  calendar
)

# init only if components.json hasn't been picked up yet (idempotent)
if [[ ! -f src/shell/ui/.shadcn-init ]]; then
  pnpm dlx shadcn@latest init --yes --base-color neutral --css-variables
  touch src/shell/ui/.shadcn-init
fi

for c in "${COMPONENTS[@]}"; do
  echo "==> shadcn add $c"
  pnpm dlx shadcn@latest add "$c" --yes $OVERWRITE
done

echo "Done. shadcn components live in src/shell/ui/."
