#!/usr/bin/env bash
# Builds an AppImage from target/<triple>/release. Usage: linux-appimage.sh <target-triple> <version> <out.AppImage>
set -euo pipefail
T=$1; V=$2; OUT=$3
D=dist/AppDir
rm -rf dist && mkdir -p "$D/usr/bin" "$D/usr/share/applications" "$D/usr/share/icons/hicolor/512x512/apps"
cp "target/$T/release/husky-forge" "target/$T/release/forge" "$D/usr/bin/"
cp crates/forge-app/assets/icon.png "$D/usr/share/icons/hicolor/512x512/apps/husky-forge.png"
cp crates/forge-app/assets/icon.png "$D/husky-forge.png"
cat > "$D/usr/share/applications/husky-forge.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Husky Forge
Comment=Image processing, perfected.
Exec=husky-forge %F
Icon=husky-forge
Categories=Graphics;Photography;
MimeType=image/jpeg;image/png;image/webp;image/tiff;image/avif;image/jxl;image/x-adobe-dng;image/x-sony-arw;image/x-canon-cr2;image/x-canon-cr3;image/x-nikon-nef;image/x-fuji-raf;inode/directory;
Terminal=false
DESK
cp "$D/usr/share/applications/husky-forge.desktop" "$D/"
cat > "$D/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/husky-forge" "$@"
RUN
chmod +x "$D/AppRun"
curl -sSL -o appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x appimagetool
ARCH=x86_64 ./appimagetool --appimage-extract-and-run "$D" "$OUT"
