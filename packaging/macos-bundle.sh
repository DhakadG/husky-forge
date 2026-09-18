#!/usr/bin/env bash
# Builds "Husky Forge.app" + dmg from target/<triple>/release. Usage: macos-bundle.sh <target-triple> <version> <out.dmg>
set -euo pipefail
T=$1; V=$2; OUT=$3
APP="dist/Husky Forge.app"
rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$T/release/husky-forge" "target/$T/release/forge" "$APP/Contents/MacOS/"
cp crates/forge-app/assets/icon.icns "$APP/Contents/Resources/icon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Husky Forge</string>
  <key>CFBundleDisplayName</key><string>Husky Forge</string>
  <key>CFBundleIdentifier</key><string>dev.huskyforge.app</string>
  <key>CFBundleVersion</key><string>$V</string>
  <key>CFBundleShortVersionString</key><string>$V</string>
  <key>CFBundleExecutable</key><string>husky-forge</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>Image</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSItemContentTypes</key><array><string>public.image</string><string>public.folder</string></array>
  </dict></array>
</dict></plist>
PLIST
# Ad-hoc signature: required on Apple silicon; users right-click → Open the first time (no Developer ID yet).
codesign --force --deep --sign - "$APP"
hdiutil create -volname "Husky Forge" -srcfolder dist -ov -format UDZO "$OUT"
