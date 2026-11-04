#!/usr/bin/env bash
# Builds release packages of minerva-cli, the command line for a Minerva server.
#
#   scripts/package-cli.sh [linux] [windows] [macos]   (default: linux windows)
#
# Each package holds the command, the license and a README. Windows is
# cross-compiled with the x86_64-pc-windows-gnu target and MinGW-w64, as for
# the server; macOS runs on a Mac and builds one binary for Apple Silicon and
# Intel.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
out="$root/dist"
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(linux windows)
mkdir -p "$out"

readme() {
  printf '%s\n' \
    "minerva-cli $version ($1)" \
    "" \
    "Everyday actions against a Minerva server, signed in with a personal API" \
    "token (in Minerva: MCP connections → Personal API tokens; a read-only token" \
    "is enough for search and backups)." \
    "" \
    "  $2" \
    "" \
    "  minerva-cli search budget review" \
    "  minerva-cli folders" \
    "  minerva-cli new Inbox \"Monday\" \"- call the bank\"" \
    "  minerva-cli append <note-id> \"One more thing\"" \
    "  minerva-cli backup ./minerva-backup      (again later: only what changed)" \
    "" \
    "Run minerva-cli --help for every command."
}

for target in "${targets[@]}"; do
  case "$target" in
  linux)
    cargo build --locked --release -p minerva-cli
    name="minerva-cli-$version-linux-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    install -m 755 target/release/minerva-cli "$stage/minerva-cli"
    cp LICENSE "$stage/"
    readme "Linux x86_64" "export MINERVA_URL=https://notes.example.com MINERVA_TOKEN=mnv_api_v1_…" >"$stage/README.txt"
    tar -C "$(dirname "$stage")" -czf "$out/$name.tar.gz" "$name"
    echo "Built $out/$name.tar.gz"
    ;;
  windows)
    cargo build --locked --release -p minerva-cli --target x86_64-pc-windows-gnu
    name="minerva-cli-$version-windows-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    cp target/x86_64-pc-windows-gnu/release/minerva-cli.exe "$stage/"
    cp LICENSE "$stage/"
    readme "Windows x86_64" "\$env:MINERVA_URL = 'https://notes.example.com'; \$env:MINERVA_TOKEN = 'mnv_api_v1_…'" | sed 's/$/\r/' >"$stage/README.txt"
    (cd "$(dirname "$stage")" && zip -qr "$out/$name.zip" "$name")
    echo "Built $out/$name.zip"
    ;;
  macos)
    cargo build --locked --release -p minerva-cli --target aarch64-apple-darwin
    cargo build --locked --release -p minerva-cli --target x86_64-apple-darwin
    name="minerva-cli-$version-macos-universal"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    lipo -create -output "$stage/minerva-cli" \
      target/aarch64-apple-darwin/release/minerva-cli \
      target/x86_64-apple-darwin/release/minerva-cli
    cp LICENSE "$stage/"
    readme "macOS" "export MINERVA_URL=https://notes.example.com MINERVA_TOKEN=mnv_api_v1_…" >"$stage/README.txt"
    tar -C "$(dirname "$stage")" -czf "$out/$name.tar.gz" "$name"
    echo "Built $out/$name.tar.gz"
    ;;
  *)
    echo "Unknown target: $target (linux, windows or macos)" >&2
    exit 1
    ;;
  esac
done
