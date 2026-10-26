#!/usr/bin/env bash
# Prepares a release: sets the version, turns the changelog's Unreleased
# section into that version, commits, and tags. Pushing the tag makes GitHub
# Actions build the Linux and Windows packages and publish the release.
#
#   scripts/release.sh 0.3.0
#   git push origin main v0.3.0
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="${1:-}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Usage: scripts/release.sh X.Y.Z" >&2
  exit 2
fi
tag="v$version"
if [ -n "$(git status --porcelain)" ]; then
  echo "Commit or stash your changes first." >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag already exists." >&2
  exit 1
fi
# The Unreleased section must say what changed.
notes="$(awk '/^## \[Unreleased\]/{on=1;next} /^## \[/{on=0} on' CHANGELOG.md | grep -v '^\s*$' || true)"
if [ -z "$notes" ]; then
  echo "CHANGELOG.md has nothing under [Unreleased]." >&2
  exit 1
fi

sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
sed -i "s/\"version\":\"[^\"]*\"/\"version\":\"$version\"/" package.json 2>/dev/null || true
cargo update --workspace --quiet
today="$(date +%Y-%m-%d)"
sed -i "s/^## \[Unreleased\]$/## [Unreleased]\n\n## [$version] - $today/" CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md package.json
git commit -m "Release $tag"
git tag -a "$tag" -m "Minerva $tag"
echo "Ready. Publish it with: git push origin $(git branch --show-current) $tag"
