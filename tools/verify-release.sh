set -euo pipefail

PUBKEY="${1:?usage: $0 <public-key-file> <artifact-file>}"
FILE="${2:?usage: $0 <public-key-file> <artifact-file>}"

if [ ! -f "$PUBKEY" ]; then
  echo "ERROR: public key not found: $PUBKEY" >&2
  exit 1
fi
if [ ! -f "$FILE" ]; then
  echo "ERROR: artifact not found: $FILE" >&2
  exit 1
fi
if ! command -v minisign >/dev/null 2>&1; then
  echo "ERROR: minisign not installed (apt install minisign)" >&2
  exit 1
fi

SIG="$FILE.minisig"
if [ ! -f "$SIG" ]; then
  echo "ERROR: signature file missing: $SIG" >&2
  echo "A genuine Zen release always ships a .minisig next to each artifact." >&2
  exit 1
fi

if minisign -V -p "$PUBKEY" -m "$FILE" -x "$SIG"; then
  echo "OK: signature valid - $(basename "$FILE") is an authentic Zen release artifact."
else
  echo "FAILED: signature INVALID for $(basename "$FILE"). Do NOT install it." >&2
  exit 1
fi
