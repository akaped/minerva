#!/usr/bin/env bash
# Builds the Docker image and release packages on this computer and publishes
# them to GitHub.
#
#   scripts/local-release.sh [all|docker|release] [--no-push]
#
#   all      (default) the Docker image, then the release packages
#   docker   only build and push the image to ghcr.io
#   release  only build the packages and publish the GitHub release
#   --no-push  build everything but upload nothing (a dry run)
#
# Image tags include :main; when HEAD carries the version
# tag (vX.Y.Z, made by scripts/release.sh) also :X.Y.Z, :X.Y and :latest.
# The release needs that tag; push it first (git push origin main vX.Y.Z), and
# the release is created from it.
#
# Not built here: the macOS desktop app, which needs a Mac. On a Mac, run
#   scripts/package-desktop.sh macos
#   gh release upload vX.Y.Z dist/minerva-desktop-*macos*
#
# Needs: cargo with the x86_64-pc-windows-gnu target, MinGW-w64
# (mingw64-gcc), WebKitGTK 4.1 and GTK 3 development packages, Node 24, zip,
# docker or podman, and the GitHub CLI signed in with package rights:
#   gh auth login && gh auth refresh -s write:packages
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

mode=all
push=1
for arg in "$@"; do
  case "$arg" in
  all | docker | release) mode="$arg" ;;
  --no-push) push=0 ;;
  -h | --help) sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "Unknown argument: $arg (try --help)" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is not installed. $2"; }

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
tag="v$version"
# owner/repo from the origin remote, lowercased as registries require.
repo="$(git remote get-url origin | sed -E 's#^(git@|https://)github\.com[:/]##; s#\.git$##')"
image="ghcr.io/${repo,,}"
released=0
git tag --points-at HEAD | grep -qx "$tag" && released=1

# ---- Checks before anything slow ------------------------------------------
step "Checking tools and repository"
need git ""
if [ "$push" = 1 ]; then
  need gh "Install it: sudo dnf install gh"
  gh auth status >/dev/null 2>&1 || die "Sign in to GitHub first: gh auth login"
fi
[ -n "$(git status --porcelain)" ] && die "Commit or stash your changes first, so the build matches a commit."

if [ "$mode" != docker ]; then
  need cargo "Install Rust: https://rustup.rs"
  need npm "Install Node.js 24."
  need zip "sudo dnf install zip"
  need x86_64-w64-mingw32-gcc "sudo dnf install mingw64-gcc"
  # With rustup, check the Windows target here; Fedora's own Rust gets it
  # from the rust-std-static-x86_64-pc-windows-gnu package instead.
  if command -v rustup >/dev/null 2>&1; then
    rustup target list --installed | grep -qx x86_64-pc-windows-gnu ||
      die "Add the Windows target: rustup target add x86_64-pc-windows-gnu"
  fi
  pkg-config --exists webkit2gtk-4.1 gtk+-3.0 2>/dev/null ||
    die "Install the desktop libraries: sudo dnf install webkit2gtk4.1-devel gtk3-devel"
  [ "$released" = 1 ] || die "HEAD is not tagged $tag. Run scripts/release.sh X.Y.Z first, or use 'docker' mode."
  grep -q "^## \[$version\]" CHANGELOG.md || die "CHANGELOG.md has no section for $version."
  if [ "$push" = 1 ]; then
    git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null ||
      die "Push the tag first: git push origin $(git branch --show-current) $tag"
  fi
fi

if [ "$mode" != release ]; then
  if command -v docker >/dev/null 2>&1; then engine=docker
  elif command -v podman >/dev/null 2>&1; then engine=podman
  else die "Install docker or podman (sudo dnf install podman)."
  fi
fi

# ---- Docker image -----------------------------------------------------------
if [ "$mode" != release ]; then
  tags=("$image:main")
  if [ "$released" = 1 ]; then
    tags+=("$image:$version" "$image:${version%.*}" "$image:latest")
  fi
  step "Building the image with $engine: ${tags[*]}"
  tag_args=()
  for t in "${tags[@]}"; do tag_args+=(-t "$t"); done
  # Podman builds OCI images by default, which silently drop HEALTHCHECK;
  # compose services that wait for service_healthy then refuse to start.
  [ "$engine" = podman ] && tag_args+=(--format docker)
  "$engine" build \
    --label "org.opencontainers.image.source=https://github.com/$repo" \
    --label "org.opencontainers.image.revision=$(git rev-parse HEAD)" \
    --label "org.opencontainers.image.version=$version" \
    "${tag_args[@]}" .

  if [ "$push" = 1 ]; then
    step "Pushing the image to ghcr.io"
    gh auth token | "$engine" login ghcr.io -u "$(gh api user -q .login)" --password-stdin
    # Podman would convert to OCI on push too, dropping HEALTHCHECK again.
    push_args=()
    [ "$engine" = podman ] && push_args=(--format v2s2)
    for t in "${tags[@]}"; do "$engine" push "${push_args[@]}" "$t"; done
  fi
fi

# ---- Release packages ---------------------------------------------------------
if [ "$mode" != docker ]; then
  step "Building and testing the web app"
  rm -rf dist
  npm ci
  npm test
  npm run build

  step "Packaging the desktop client (Linux, Windows)"
  scripts/package-desktop.sh linux windows

  step "Packaging the server (Linux, Windows)"
  scripts/package-server.sh linux windows

  step "Packaging minerva-cli (Linux, Windows)"
  scripts/package-cli.sh linux windows

  step "Packaging the web clipper"
  npm run build:clipper
  mv dist/clipper/*.zip dist/
  rm -rf dist/clipper

  step "Checksums and release notes"
  (cd dist && sha256sum -- * >SHA256SUMS.txt)
  notes="$(mktemp)"
  trap 'rm -f "$notes"' EXIT
  awk -v v="$version" '
    index($0, "## [" v "]") == 1 { on = 1; next }
    /^## \[/ { on = 0 }
    on' CHANGELOG.md >"$notes"
  printf '\n**Downloads:** `minerva-desktop` is the app for your computer (Linux, Windows or macOS); `minerva-server` runs your own server; `minerva-cli` searches, adds notes and makes backups from a terminal; `minerva-clipper` is the web clipper for the Chrome Web Store and addons.mozilla.org (see `docs/clipper.md`); or use the Docker image `%s:%s`. Check downloads against `SHA256SUMS.txt`.\n' \
    "$image" "$version" >>"$notes"
  ls -lh dist

  if [ "$push" = 1 ]; then
    step "Publishing release $tag"
    if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
      gh release upload "$tag" dist/* --repo "$repo" --clobber
      gh release edit "$tag" --repo "$repo" --notes-file "$notes"
    else
      gh release create "$tag" dist/* --repo "$repo" --verify-tag \
        --title "Minerva $tag" --notes-file "$notes"
    fi
  fi
fi

step "Done"
[ "$push" = 1 ] || echo "Nothing was uploaded (--no-push). Packages are in dist/."
