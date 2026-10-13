#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
app="$PWD/src-tauri/target/release/bundle/macos/DashBeam.app"
extension="$app/Contents/PlugIns/DashBeamShare.appex"
test -x "$app/Contents/MacOS/DashBeam"
if ! test -x "$extension/Contents/MacOS/DashBeamShare"; then
  echo "Missing Share extension. Bundle with src-tauri/tauri.local-macos.conf.json before packaging." >&2
  exit 1
fi
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$extension/Contents/Info.plist")" = com.dashbeam.desktop.share
test "$(/usr/libexec/PlistBuddy -c 'Print :NSExtension:NSExtensionPointIdentifier' "$extension/Contents/Info.plist")" = com.apple.share-services
codesign --verify --strict "$extension"
codesign --verify --deep --strict "$app"
mkdir -p dist
ditto -c -k --sequesterRsrc --keepParent "$app" dist/DashBeam-0.7.1-local-macos-arm64.zip
