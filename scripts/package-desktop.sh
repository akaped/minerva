#!/usr/bin/env bash
# Builds release packages of the Minerva desktop client.
#
#   scripts/package-desktop.sh [linux] [windows]     (default: both)
#
# Linux needs the WebKitGTK 4.1 and GTK 3 development packages.
# Windows is cross-compiled with the x86_64-pc-windows-gnu target and MinGW-w64
# (rustup target add x86_64-pc-windows-gnu; mingw64-gcc / gcc-mingw-w64-x86-64).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
out="$root/dist"
assets="$root/crates/minerva-desktop/assets"
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(linux windows)
mkdir -p "$out"

for target in "${targets[@]}"; do
  case "$target" in
  linux)
    cargo build --locked --release -p minerva-desktop
    name="minerva-desktop-$version-linux-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    install -m 755 target/release/minerva "$stage/minerva"
    install -m 644 "$assets/minerva.desktop" "$assets/minerva.png" "$stage/"
    cat >"$stage/install.sh" <<'EOF'
#!/bin/sh
# Installs Minerva for the current user (~/.local).
set -e
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
install -Dm755 "$here/minerva" "$prefix/bin/minerva"
install -Dm644 "$here/minerva.desktop" "$prefix/share/applications/minerva.desktop"
install -Dm644 "$here/minerva.png" "$prefix/share/icons/hicolor/256x256/apps/minerva.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "Installed to $prefix/bin/minerva"
EOF
    chmod 755 "$stage/install.sh"
    tar -C "$(dirname "$stage")" -czf "$out/$name.tar.gz" "$name"
    echo "Built $out/$name.tar.gz"
    ;;
  windows)
    cargo build --locked --release -p minerva-desktop --target x86_64-pc-windows-gnu
    name="minerva-desktop-$version-windows-x86_64"
    stage="$(mktemp -d)/$name"
    mkdir -p "$stage"
    cp target/x86_64-pc-windows-gnu/release/minerva.exe "$stage/Minerva.exe"
    # The GNU target loads WebView2Loader.dll at runtime; ship it next to the executable.
    loader="$(ls -t target/x86_64-pc-windows-gnu/release/build/webview2-com-sys-*/out/x64/WebView2Loader.dll | head -1)"
    cp "$loader" "$stage/"
    printf '%s\r\n' \
      "Minerva desktop client $version" \
      "" \
      "Run Minerva.exe. Keep WebView2Loader.dll in the same folder." \
      "Requires the Microsoft Edge WebView2 Runtime, included with Windows 10 and 11:" \
      "https://developer.microsoft.com/microsoft-edge/webview2/" \
      "" \
      "Sign-in tokens are stored in Windows Credential Manager; server addresses in" \
      "%APPDATA%\\minerva\\connections.json." >"$stage/README.txt"
    (cd "$(dirname "$stage")" && rm -f "$out/$name.zip" && zip -qr "$out/$name.zip" "$name")
    echo "Built $out/$name.zip"
    ;;
  *)
    echo "Unknown target: $target (expected linux or windows)" >&2
    exit 2
    ;;
  esac
done
