#!/usr/bin/env bash
# Run the stdio bridge PoC end to end against a real CLI binary.
#
#   ./docs/stdio-bridge-poc/run_poc.sh /path/to/zen <cli-rev> [runs] [out.json]
#
# Starts the localhost mock LLM stub (127.0.0.1:18123), builds the PoC
# example, runs it N times against the given binary, then stops the stub.
# The stub never touches the network; the only key in play is a dummy
# `ZEN_API_KEY` value (`poc-dummy-key`) set by the PoC itself.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:?usage: run_poc.sh /path/to/zen <cli-rev> [runs] [out.json]}"
CLI_REV="${2:?usage: run_poc.sh /path/to/zen <cli-rev> [runs] [out.json]}"
RUNS="${3:-5}"
OUT="${4:-$ROOT/stdio-bridge-poc-run.json}"

export PATH="$HOME/.cargo/bin:$PATH"
cd "$ROOT/engine"

python3 "$ROOT/docs/stdio-bridge-poc/mock_llm_stub.py" 18123 \
  > /tmp/stdio-poc-stub.log 2>&1 &
STUB_PID=$!
trap 'kill $STUB_PID 2>/dev/null || true' EXIT

# Wait for the stub to answer (it is stdlib http.server; < 5 s expected).
for _ in $(seq 1 50); do
  if curl -s -o /dev/null -X POST http://127.0.0.1:18123/v1/chat/completions \
      -H 'Content-Type: application/json' -d '{}'; then
    break
  fi
  sleep 0.1
done
echo "mock stub up (pid $STUB_PID)"

POC_BASE_URL=http://127.0.0.1:18123/v1 \
POC_RUNS="$RUNS" POC_CLI_REV="$CLI_REV" POC_OUT="$OUT" \
  cargo run --offline --locked --example stdio_bridge_poc -- "$BIN"

echo "stub request log:"
cat /tmp/stdio-poc-stub.log
