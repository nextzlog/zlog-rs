#!/bin/bash
set -eu
[ "$GITHUB_REF" = refs/heads/master ] || { echo "publishing only from master"; exit 0; }
echo "$GITHUB_SHA" > assets/version.txt
gh release create "$TAG" --prerelease --title "$TAG" --notes "Built from master." --target "$GITHUB_SHA" || true
gh release upload "$TAG" assets/* --clobber
gh api -X PATCH "repos/$GITHUB_REPOSITORY/git/refs/tags/$TAG" -f sha="$GITHUB_SHA" -F force=true
