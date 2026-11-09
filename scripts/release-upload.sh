#!/usr/bin/env bash
# Attach build outputs to a GitHub release, creating the release first when it does not exist.
#
# Usage: scripts/release-upload.sh <tag> <target-commit> <file>...
# Needs GH_TOKEN (and GH_REPO outside a checkout).
#
# It never edits an existing release: in the v0.2.0 run the Actions token (contents: write) was
# refused the update-release call ("Resource not accessible by integration") by the second job,
# while creating the release and uploading assets worked. Each job only creates (if missing) and
# uploads, so the jobs can finish in any order.
set -euo pipefail
TAG="$1"
TARGET="$2"
shift 2
NOTES="docs/releases/$TAG.md"

if ! gh release view "$TAG" >/dev/null 2>&1; then
  args=(--target "$TARGET" --title "LOUPE $TAG")
  if [[ -f "$NOTES" ]]; then
    args+=(--notes-file "$NOTES")
  else
    args+=(--notes "")
  fi
  # Another job may have created it in the meantime; that is fine.
  gh release create "$TAG" "${args[@]}" || gh release view "$TAG" >/dev/null
fi
gh release upload "$TAG" "$@" --clobber
echo "Uploaded to $TAG: $*"
