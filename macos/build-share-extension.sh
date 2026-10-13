#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Tauri calls this before bundling, then copies the signed extension via
# bundle.macOS.files. Keep generated files outside the source tree.
extension="$PWD/src-tauri/target/share-extension/DashBeamShare.appex"
mkdir -p "$extension/Contents/MacOS" "$extension/Contents/Resources"
xcrun swiftc -O -application-extension -target arm64-apple-macos13.0 -module-name DashBeamShare \
  macos/ShareExtension/main.swift -o "$extension/Contents/MacOS/DashBeamShare" \
  -framework AppKit -framework Foundation -framework UniformTypeIdentifiers
cp macos/ShareExtension/Info.plist "$extension/Contents/Info.plist"
cp src-tauri/icons/icon.icns "$extension/Contents/Resources/icon.icns"
codesign --force --sign - --entitlements macos/ShareExtension/entitlements.plist "$extension"
codesign --verify --strict "$extension"
