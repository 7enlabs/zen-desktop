set -euo pipefail

TAG="${1:?usage: $0 <tag vX.Y.Z> <dist-dir>}"
DIST="${2:?usage: $0 <tag vX.Y.Z> <dist-dir>}"
VERSION="${TAG#v}"

: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY is not set (protected CI variable; is this a protected tag?)}"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONF="$ROOT/desktop/src-tauri/tauri.conf.json"
APP_VERSION="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$CONF" | tail -n 1)"
if [ "$APP_VERSION" != "$VERSION" ]; then
  echo "ERROR: tag $TAG does not match tauri.conf.json version $APP_VERSION" >&2
  exit 1
fi

PROJECT_URL="${CI_API_V4_URL:-https://gitlab.com/api/v4}/projects/${CI_PROJECT_ID:-87760189}"
PKG_URL="$PROJECT_URL/packages/generic/zen-desktop/$TAG"

(cd "$ROOT/desktop/src-tauri" && cargo tauri build --bundles appimage)

BUNDLE="$ROOT/desktop/src-tauri/target/release/bundle/appimage"
SRC="$(find "$BUNDLE" -maxdepth 1 -name '*.AppImage' | head -n 1)"
[ -n "$SRC" ] || { echo "ERROR: no AppImage in $BUNDLE" >&2; exit 1; }
ARCH="$(basename "$SRC" .AppImage | sed 's/.*_//')"
NAME="zen-desktop_${VERSION}_${ARCH}.AppImage"

mkdir -p "$DIST"
cp "$SRC" "$DIST/$NAME"
(cd "$ROOT/desktop/src-tauri" && cargo tauri signer sign "$DIST/$NAME")
SIG_FILE="$DIST/$NAME.sig"
[ -s "$SIG_FILE" ] || { echo "ERROR: signer wrote no $SIG_FILE" >&2; exit 1; }

if ! base64 -d < "$SIG_FILE" | grep -q "^trusted comment: .*file:$NAME\$"; then
  echo "ERROR: signature trusted comment does not name $NAME" >&2
  base64 -d < "$SIG_FILE" | sed -n 's/^trusted comment: /  got: /p' >&2
  exit 1
fi

PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SIGNATURE="$(tr -d '\n' < "$SIG_FILE")"
cat > "$DIST/latest.json" <<EOF
{
  "version": "$VERSION",
  "notes": "ZenCode Desktop $TAG - see CHANGELOG.md",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "linux-x86_64": {
      "signature": "$SIGNATURE",
      "url": "$PKG_URL/$NAME"
    }
  }
}
EOF
echo "OK: $DIST/$NAME (+ .sig) and $DIST/latest.json for $TAG"
