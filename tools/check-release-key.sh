set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PUB_FILE="$ROOT/docs/keys/zen-release.pub"
CONF="$ROOT/desktop/src-tauri/tauri.conf.json"

[ -f "$PUB_FILE" ] || { echo "FAIL: missing $PUB_FILE"; exit 1; }
[ -f "$CONF" ] || { echo "FAIL: missing $CONF"; exit 1; }

expected="$(cat "$PUB_FILE")"

b64="$(sed -n 's/.*"pubkey"[[:space:]]*:[[:space:]]*"\([^"]\{10,\}\)".*/\1/p' "$CONF" | head -n 1)"
[ -n "$b64" ] || { echo "FAIL: no updater pubkey found in $CONF"; exit 1; }

actual="$(printf '%s' "$b64" | base64 -d 2>/dev/null)" \
  || { echo "FAIL: updater pubkey is not valid base64"; exit 1; }

case "$actual" in
  *PLACEHOLDER*|*TODO*|*CHANGEME*|*changeme*|*example*|*EXAMPLE*)
    echo "FAIL: updater pubkey is still a placeholder"; exit 1 ;;
esac
case "$actual" in
  RW*|untrusted\ comment:*)
    # Real minisign public key: either the bare RW... line or the
    # two-line file format ("untrusted comment: ..." + "RW...").
    ;;
  *) echo "FAIL: updater pubkey is not a minisign public key"; exit 1 ;;
esac

if [ "$actual" = "$expected" ]; then
  keyid="$(printf '%s' "$actual" | head -n 1 | sed 's/.*key //')"
  echo "OK: tauri.conf.json updater pubkey matches docs/keys/zen-release.pub ($keyid)"
else
  echo "FAIL: updater pubkey in $CONF does not match $PUB_FILE"
  echo "  tauri.conf.json decodes to: $(printf '%s' "$actual" | head -n 1)"
  echo "  docs/keys/zen-release.pub: $(printf '%s' "$expected" | head -n 1)"
  exit 1
fi
