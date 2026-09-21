#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
app="$PWD/src-tauri/target/release/bundle/macos/DashBeam.app"
extension="$app/Contents/PlugIns/DashBeamShare.appex"
test -x "$app/Contents/MacOS/DashBeam"
mkdir -p "$extension/Contents/MacOS" "$extension/Contents/Resources"
xcrun swiftc -O -application-extension -target arm64-apple-macos13.0 -module-name DashBeamShare \
  macos/ShareExtension/main.swift -o "$extension/Contents/MacOS/DashBeamShare" \
  -framework AppKit -framework Foundation -framework UniformTypeIdentifiers
cp macos/ShareExtension/Info.plist "$extension/Contents/Info.plist"
cp src-tauri/icons/icon.icns "$extension/Contents/Resources/icon.icns"
codesign --force --sign - --entitlements macos/ShareExtension/entitlements.plist "$extension"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
mkdir -p dist
ditto -c -k --sequesterRsrc --keepParent "$app" dist/DashBeam-0.7.1-local-macos-arm64.zip
