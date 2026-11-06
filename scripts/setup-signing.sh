#!/usr/bin/env bash
# Creates a self-signed code-signing identity, "Strata Local Signing", in the login keychain.
#
# Why: an ad-hoc signature changes with every build, and macOS ties keychain access to the
# signature, so each rebuild asked for the keychain password again. Builds signed with one
# stable identity keep "Always Allow". Free and local; not a Developer ID (no notarization).
# Safe to run twice: it does nothing when the identity already exists.
set -euo pipefail
name="Strata Local Signing"
keychain="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$name" "$keychain" >/dev/null 2>&1; then
  echo "\"$name\" already exists."
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cat > "$work/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF
# /usr/bin/openssl (LibreSSL) writes a PKCS#12 that `security import` reads on every macOS.
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$work/key.pem" -out "$work/cert.pem" -config "$work/cert.cnf" 2>/dev/null
pass="$(/usr/bin/openssl rand -hex 16)"
/usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
  -name "$name" -out "$work/identity.p12" -passout "pass:$pass"
# -T lets codesign use the key without asking each time.
security import "$work/identity.p12" -k "$keychain" -P "$pass" -T /usr/bin/codesign >/dev/null
echo "Created \"$name\". Builds signed with it keep keychain access across rebuilds."
