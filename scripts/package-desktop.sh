#!/usr/bin/env bash
# Builds release packages of the Minerva desktop client.
#
#   scripts/package-desktop.sh [linux] [windows] [macos]   (default: linux windows)
#
# Linux needs the WebKitGTK 4.1 and GTK 3 development packages.
# Windows is cross-compiled with the x86_64-pc-windows-gnu target and MinGW-w64
# (rustup target add x86_64-pc-windows-gnu; mingw64-gcc / gcc-mingw-w64-x86-64).
# macOS runs on a Mac (Xcode command-line tools) and builds a universal
# Minerva.app for Apple Silicon and Intel
# (rustup target add aarch64-apple-darwin x86_64-apple-darwin).
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
# The full path: desktop sessions often leave ~/.local/bin out of PATH.
mkdir -p "$prefix/share/applications"
sed "s|^Exec=.*|Exec=$prefix/bin/minerva|" "$here/minerva.desktop" >"$prefix/share/applications/minerva.desktop"
chmod 644 "$prefix/share/applications/minerva.desktop"
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
  macos)
    for arch in aarch64 x86_64; do
      cargo build --locked --release -p minerva-desktop --target "$arch-apple-darwin"
    done
    name="minerva-desktop-$version-macos-universal"
    stage="$(mktemp -d)"
    app="$stage/Minerva.app/Contents"
    mkdir -p "$app/MacOS" "$app/Resources"
    lipo -create -output "$app/MacOS/minerva" \
      target/aarch64-apple-darwin/release/minerva \
      target/x86_64-apple-darwin/release/minerva
    # The icon, at every size macOS asks for.
    iconset="$stage/minerva.iconset"
    mkdir -p "$iconset"
    for size in 16 32 64 128 256; do
      sips -z $size $size "$assets/minerva.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
      half=$((size / 2))
      if [ $half -ge 16 ]; then
        cp "$iconset/icon_${size}x${size}.png" "$iconset/icon_${half}x${half}@2x.png"
      fi
    done
    iconutil -c icns "$iconset" -o "$app/Resources/minerva.icns"
    cat >"$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Minerva</string>
  <key>CFBundleDisplayName</key><string>Minerva</string>
  <key>CFBundleIdentifier</key><string>io.github.akaped.minerva</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleExecutable</key><string>minerva</string>
  <key>CFBundleIconFile</key><string>minerva</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
</dict>
</plist>
PLIST
    # Not notarized: an ad-hoc signature lets it run on Apple Silicon.
    codesign --force --deep --sign - "$stage/Minerva.app"
    printf '%s\n' \
      "Minerva desktop client $version for macOS 11 or later (Apple Silicon and Intel)" \
      "" \
      "Drag Minerva.app to Applications. The app is not notarized by Apple, so" \
      "the first time, right-click (or Control-click) it and choose Open, then" \
      "Open again. Or run once in Terminal:" \
      "  xattr -dr com.apple.quarantine /Applications/Minerva.app" \
      "" \
      "Sign-in tokens are stored in your Keychain." >"$stage/README.txt"
    rm -f "${out:?}/${name:?}.zip"
    (cd "$stage" && zip -qry "$out/$name.zip" Minerva.app README.txt)
    echo "Built $out/$name.zip"
    ;;
  *)
    echo "Unknown target: $target (expected linux, windows or macos)" >&2
    exit 2
    ;;
  esac
done
