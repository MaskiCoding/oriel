#!/bin/sh
set -eu
profile="${1:-debug}"
sign_id="${ORIEL_SIGN_ID:-Oriel Dev}"
app="dist/Oriel.app"

if [ "$profile" = "release" ]; then
  cargo build --release
else
  cargo build
fi
./scripts/mkicons.sh dist

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/oriel" "$app/Contents/MacOS/oriel"
cp dist/oriel.icns dist/MenubarTemplate.png dist/MenubarTemplate@2x.png "$app/Contents/Resources/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleIdentifier</key><string>com.maskicoding.oriel</string>
	<key>CFBundleName</key><string>Oriel</string>
	<key>CFBundleExecutable</key><string>oriel</string>
	<key>CFBundleIconFile</key><string>oriel</string>
	<key>CFBundleShortVersionString</key><string>$version</string>
	<key>CFBundleVersion</key><string>$version</string>
	<key>LSMinimumSystemVersion</key><string>26.0</string>
	<key>LSUIElement</key><true/>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

codesign --force --sign "$sign_id" "$app"
echo "bundled $app ($profile, signed '$sign_id')"
