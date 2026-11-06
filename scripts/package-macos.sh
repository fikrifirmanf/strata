#!/usr/bin/env bash
# Builds a universal (Apple silicon + Intel) Strata.app and wraps it in a .dmg under dist/.
set -euo pipefail
version="${1:?usage: package-macos.sh <version>}"

cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

stage="dist/dmg"
app="$stage/Strata.app"
rm -rf dist && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
ln -s /Applications "$stage/Applications"
lipo -create -output "$app/Contents/MacOS/strata" \
  target/aarch64-apple-darwin/release/strata target/x86_64-apple-darwin/release/strata
cp assets/icon/strata.icns "$app/Contents/Resources/strata.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Strata</string>
  <key>CFBundleDisplayName</key><string>Strata</string>
  <key>CFBundleIdentifier</key><string>dev.strata.app</string>
  <key>CFBundleExecutable</key><string>strata</string>
  <key>CFBundleIconFile</key><string>strata</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Signed with "Strata Local Signing" when this Mac has it (scripts/setup-signing.sh), so updates
# keep keychain access; ad-hoc otherwise (required on Apple silicon). Not notarized, so the
# first launch needs right-click → Open.
"$(dirname "$0")/sign.sh" "$app"

hdiutil create -volname "Strata ${version}" -srcfolder "$stage" -ov -format UDZO "dist/Strata-${version}-macos-universal.dmg"
rm -rf "$stage"
