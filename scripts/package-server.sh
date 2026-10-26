#!/usr/bin/env bash
# Builds release packages of the Minerva server.
#
#   scripts/package-server.sh [linux] [windows] [macos]   (default: linux windows)
#
# Each package holds the server (the web app, themes and PDF templates are
# built in), a settings example, the changelog, the license and a README.
# Windows is cross-compiled with the x86_64-pc-windows-gnu target and MinGW-w64
# (rustup target add x86_64-pc-windows-gnu; mingw64-gcc / gcc-mingw-w64-x86-64).
# macOS runs on a Mac and builds one binary for Apple Silicon and Intel
# (rustup target add aarch64-apple-darwin x86_64-apple-darwin).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
out="$root/dist"
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(linux windows)
mkdir -p "$out"

# The settings the server reads, from the Docker example (whose Compose-only
# settings do nothing here).
settings() {
  printf '%s\n' \
    "# Minerva server settings. Set them in the environment before starting the" \
    "# server; the Docker-only settings (MINERVA_PORT, MINERVA_IMAGE," \
    "# MINERVA_DOMAIN, COMPOSE_PROFILES, TUNNEL_TOKEN) do nothing here." \
    "# Add MINERVA_BIND (default 127.0.0.1:3000) and MINERVA_DATABASE" \
    "# (default data/minerva.db) as needed." \
    ""
  cat .env.example
}

for target in "${targets[@]}"; do
  case "$target" in
  linux)
    cargo build --locked --release -p minerva-server
    name="minerva-server-$version-linux-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    install -m 755 target/release/minerva-server "$stage/minerva-server"
    settings >"$stage/minerva.env.example"
    cp CHANGELOG.md LICENSE "$stage/"
    cat >"$stage/README.txt" <<EOF
Minerva server $version for Linux (x86_64)

First start (the password is needed only once, to create the administrator):

  MINERVA_ADMIN_EMAIL=you@example.com \\
  MINERVA_ADMIN_PASSWORD='at-least-12-characters' \\
  ./minerva-server

Then open http://127.0.0.1:3000. Data is kept in data/minerva.db; set
MINERVA_DATABASE to keep it elsewhere and MINERVA_BIND to listen on another
address. minerva.env.example lists every setting, including email.

For a public server, put it behind HTTPS (a reverse proxy or Cloudflare Tunnel)
and set MINERVA_SECURE_COOKIE=1 and MINERVA_PUBLIC_URL. The Docker image
ghcr.io/akaped/minerva is the easiest way to run it in production.
EOF
    tar -C "$(dirname "$stage")" -czf "$out/$name.tar.gz" "$name"
    echo "Built $out/$name.tar.gz"
    ;;
  windows)
    cargo build --locked --release -p minerva-server --target x86_64-pc-windows-gnu
    name="minerva-server-$version-windows-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    cp target/x86_64-pc-windows-gnu/release/minerva-server.exe "$stage/"
    settings | sed 's/$/\r/' >"$stage/minerva.env.example"
    cp CHANGELOG.md LICENSE "$stage/"
    printf '%s\r\n' \
      "Minerva server $version for Windows (x86_64)" \
      "" \
      "First start, in PowerShell (the password is needed only once, to create" \
      "the administrator):" \
      "" \
      "  \$env:MINERVA_ADMIN_EMAIL = 'you@example.com'" \
      "  \$env:MINERVA_ADMIN_PASSWORD = 'at-least-12-characters'" \
      "  .\\minerva-server.exe" \
      "" \
      "Then open http://127.0.0.1:3000. Data is kept in data\\minerva.db; set" \
      "MINERVA_DATABASE to keep it elsewhere and MINERVA_BIND to listen on another" \
      "address. minerva.env.example lists every setting, including email." >"$stage/README.txt"
    (cd "$(dirname "$stage")" && rm -f "$out/$name.zip" && zip -qr "$out/$name.zip" "$name")
    echo "Built $out/$name.zip"
    ;;
  macos)
    for arch in aarch64 x86_64; do
      cargo build --locked --release -p minerva-server --target "$arch-apple-darwin"
    done
    name="minerva-server-$version-macos-universal"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    lipo -create -output "$stage/minerva-server" \
      target/aarch64-apple-darwin/release/minerva-server \
      target/x86_64-apple-darwin/release/minerva-server
    codesign --force --sign - "$stage/minerva-server"
    settings >"$stage/minerva.env.example"
    cp CHANGELOG.md LICENSE "$stage/"
    cat >"$stage/README.txt" <<EOF
Minerva server $version for macOS 11 or later (Apple Silicon and Intel)

The program is not notarized by Apple. Allow it once with:

  xattr -d com.apple.quarantine minerva-server

First start (the password is needed only once, to create the administrator):

  MINERVA_ADMIN_EMAIL=you@example.com \\
  MINERVA_ADMIN_PASSWORD='at-least-12-characters' \\
  ./minerva-server

Then open http://127.0.0.1:3000. Data is kept in data/minerva.db; set
MINERVA_DATABASE to keep it elsewhere and MINERVA_BIND to listen on another
address. minerva.env.example lists every setting, including email.
EOF
    tar -C "$(dirname "$stage")" -czf "$out/$name.tar.gz" "$name"
    echo "Built $out/$name.tar.gz"
    ;;
  *)
    echo "Unknown target: $target (expected linux, windows or macos)" >&2
    exit 2
    ;;
  esac
done
