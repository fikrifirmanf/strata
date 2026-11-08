#!/usr/bin/env bash
# Builds the x86_64 Linux binary and packs it with a desktop entry and icon under dist/.
set -euo pipefail
version="${1:?usage: package-linux.sh <version>}"

cargo build --release
name="strata-${version}-linux-x86_64"
stage="dist/${name}"
rm -rf dist && mkdir -p "$stage"
cp target/release/strata "$stage/strata"
cp assets/icon/strata.png "$stage/strata.png"
cp README.md LICENSE THIRD_PARTY_NOTICES.md "$stage/"
mkdir -p "$stage/licenses"
cp assets/fonts/OFL.txt "$stage/licenses/FONTS-OFL.txt"
cp assets/db/README.md "$stage/licenses/DATABASE-LOGOS.md"
cat > "$stage/strata.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Strata
Comment=Native database workbench
Exec=strata %U
Icon=strata
Categories=Development;Database;
StartupWMClass=dev.strata.app
DESKTOP
tar -C dist -czf "dist/${name}.tar.gz" "$name"
rm -rf "$stage"
