#!/usr/bin/env bash
# Signs a Strata binary or .app with "Strata Local Signing" when it exists (see
# setup-signing.sh), else ad-hoc. One stable signature keeps "Always Allow" for the keychain.
set -euo pipefail
target="${1:?usage: sign.sh <binary or .app>}"
name="Strata Local Signing"
identity="-"
if security find-certificate -c "$name" >/dev/null 2>&1; then
  identity="$name"
fi
deep=()
[[ "$target" == *.app ]] && deep=(--deep)
codesign --force ${deep[@]+"${deep[@]}"} --identifier dev.strata.app --sign "$identity" "$target"
