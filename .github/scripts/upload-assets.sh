#!/usr/bin/env bash
# Upload FILES to the release with id RELEASE_ID, replacing any asset of the
# same name (so re-runs are safe).
#
#   upload-assets.sh RELEASE_ID FILE...     (needs GH_TOKEN, GH_REPO)
set -euo pipefail
id="$1"
shift
for f in "$@"; do
  name="$(basename "$f")"
  old="$(gh api "repos/$GH_REPO/releases/$id/assets?per_page=100" \
    --jq ".[] | select(.name == \"$name\") | .id")"
  if [ -n "$old" ]; then
    gh api --method DELETE "repos/$GH_REPO/releases/assets/$old"
  fi
  gh api --method POST -H "Content-Type: application/octet-stream" \
    "https://uploads.github.com/repos/$GH_REPO/releases/$id/assets?name=$name" \
    --input "$f" --jq '"uploaded \(.name) (\(.size) bytes)"'
done
