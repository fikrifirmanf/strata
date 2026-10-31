#!/usr/bin/env bash
# Reads the version from Cargo.toml, checks it against the pushed tag, and exposes it as the
# `version` step output.
set -euo pipefail
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
if [ "${GITHUB_REF_NAME#v}" != "$version" ]; then
  echo "Tag $GITHUB_REF_NAME does not match Cargo.toml version $version" >&2
  exit 1
fi
echo "version=$version" >> "$GITHUB_OUTPUT"
