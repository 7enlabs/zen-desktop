set -euo pipefail

VERSION="${1:?usage: MINISIGN_KEY=<keyfile> $0 <version> <dist-dir>}"
DIST="${2:?usage: MINISIGN_KEY=<keyfile> $0 <version> <dist-dir>}"
KEY="${MINISIGN_KEY:?MINISIGN_KEY env var must point at the release secret key}"

if [ ! -f "$KEY" ]; then
  echo "ERROR: secret key not found: $KEY" >&2
  exit 1
fi
if [ ! -d "$DIST" ]; then
  echo "ERROR: dist dir not found: $DIST" >&2
  exit 1
fi
if ! command -v minisign >/dev/null 2>&1; then
  echo "ERROR: minisign not installed (apt install minisign)" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_CONF="$REPO_ROOT/desktop/src-tauri/tauri.conf.json"
RELEASE_PUB="$REPO_ROOT/docs/keys/zencode-release.pub"
if [ ! -f "$TAURI_CONF" ] || [ ! -f "$RELEASE_PUB" ]; then
  echo "ERROR: missing $TAURI_CONF or $RELEASE_PUB" >&2
  exit 1
fi
if grep -q "PLACEHOLDER" "$TAURI_CONF"; then
  echo "ERROR: updater pubkey in tauri.conf.json is still PLACEHOLDER." >&2
  echo "Generate the release keypair and publish the public key first" >&2
  echo "(see docs/signed-releases.md, blocker for Z App)." >&2
  exit 1
fi

echo "Signing release $VERSION in $DIST ..."
count=0
for f in "$DIST"/*; do
  [ -f "$f" ] || continue
  case "$f" in
    *.minisig|*SHA256SUMS*) continue ;;
  esac
  minisign -S -s "$KEY" -m "$f" \
    -t "zen-desktop $VERSION - $(basename "$f")" \
    -x "$f.minisig"

  minisign -V -q -p "$RELEASE_PUB" -m "$f" -x "$f.minisig"
  echo "  signed: $(basename "$f")"
  count=$((count + 1))
done

if [ "$count" -eq 0 ]; then
  echo "ERROR: no artifacts found in $DIST" >&2
  exit 1
fi

(
  cd "$DIST"
  find . -maxdepth 1 -type f ! -name '*.minisig' ! -name 'SHA256SUMS*' -printf '%f\0' \
    | sort -z | xargs -0 sha256sum -- > SHA256SUMS
)
minisign -S -s "$KEY" -m "$DIST/SHA256SUMS" \
  -t "zen-desktop $VERSION - SHA256SUMS" \
  -x "$DIST/SHA256SUMS.minisig"
minisign -V -q -p "$RELEASE_PUB" -m "$DIST/SHA256SUMS" -x "$DIST/SHA256SUMS.minisig"

echo "OK: signed $count artifact(s) + SHA256SUMS for $VERSION"
echo "Upload to the release page: artifacts, *.minisig, SHA256SUMS, SHA256SUMS.minisig"
