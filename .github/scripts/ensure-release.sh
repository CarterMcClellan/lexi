#!/usr/bin/env bash
# Print the id of the GitHub release for TAG (drafts included), waiting up to
# WAIT seconds for another workflow to create it, else creating a draft.
#
#   ensure-release.sh TAG [WAIT]     (needs GH_TOKEN, GH_REPO; optional RELEASE_BODY)
#
# Every release workflow resolves the release through this, so a tag gets
# exactly one draft (tauri-action's own find-or-create, run from parallel jobs,
# made two for v0.2.0 — the tag lookup it uses can't see drafts).
set -euo pipefail
tag="$1"
wait="${2:-0}"

find_release() {
  gh api "repos/$GH_REPO/releases?per_page=100" \
    --jq "[.[] | select(.tag_name == \"$tag\")] | sort_by(.id) | .[0].id // empty"
}

deadline=$((SECONDS + wait))
while :; do
  id="$(find_release)"
  if [ -n "$id" ]; then
    echo "$id"
    exit 0
  fi
  [ "$SECONDS" -ge "$deadline" ] && break
  sleep 15
done

gh api --method POST "repos/$GH_REPO/releases" \
  -f tag_name="$tag" -f name="Lexi $tag" -F draft=true \
  -f body="${RELEASE_BODY:-Automated build.}" --jq .id
