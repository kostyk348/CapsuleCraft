#!/usr/bin/env bash
# Verify a CapsuleCraft release artifact.
#   ./verify-release.sh v2.12.5 [artifact-file]
set -euo pipefail
TAG="${1:?usage: verify-release.sh <tag> [artifact]}"
ART="${2:-}"
REPO="kostyk348/CapsuleCraft"
BASE="https://github.com/${REPO}/releases/download/${TAG}"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

echo "== downloading SHA256SUMS + signatures for ${TAG}"
curl -sSfL -o "$WORK/SHA256SUMS.txt"      "${BASE}/SHA256SUMS.txt"
curl -sSfL -o "$WORK/SHA256SUMS.txt.bundle" "${BASE}/SHA256SUMS.txt.bundle"

echo "== cosign keyless signature over the checksum file"
cosign verify-blob \
  --bundle "$WORK/SHA256SUMS.txt.bundle" \
  --certificate-identity-regexp "https://github.com/${REPO}/.github/workflows/release.yml@.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "$WORK/SHA256SUMS.txt"
echo "   signature OK"

if curl -sSfL -o "$WORK/SHA256SUMS.txt.asc" "${BASE}/SHA256SUMS.txt.asc" 2>/dev/null; then
  echo "== GPG signature over the checksum file"
  gpg --verify "$WORK/SHA256SUMS.txt.asc" "$WORK/SHA256SUMS.txt"
fi

if [ -n "$ART" ]; then
  echo "== checksum of ${ART}"
  EXPECTED="$(grep -E "  ${ART}$" "$WORK/SHA256SUMS.txt" | awk '{print $1}')"
  [ -n "$EXPECTED" ] || { echo "artifact ${ART} is not listed in SHA256SUMS.txt"; exit 1; }
  ACTUAL="$(sha256sum "$ART" | awk '{print $1}')"
  [ "$EXPECTED" = "$ACTUAL" ] || { echo "MISMATCH: expected $EXPECTED, got $ACTUAL"; exit 1; }
  echo "   checksum OK"
fi
echo "All verifications passed."
