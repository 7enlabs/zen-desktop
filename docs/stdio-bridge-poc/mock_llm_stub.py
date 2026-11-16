#!/usr/bin/env python3
"""Local mock LLM stub for the stdio bridge PoC (dev-only, localhost-only).

Serves the exact canned OpenAI-compatible reply the CLI repo's own mock
uses (`crates/zen-cli/tests/common/mod.rs`, `Reply::text`): one assistant
message, "Hello from the mock LLM.", usage 7 prompt / 3 completion /
10 total tokens, model `mock-model`. Every POST gets the same
byte-identical response; the stub never calls the outside network.

Binds only 127.0.0.1:18123. Logs method + path + a sequence number only
(no request bodies, no headers, no keys).

Usage:
    python3 mock_llm_stub.py [port]      # default 18123
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

BODY = json.dumps({
    "id": "chatcmpl-mock",
    "object": "chat.completion",
    "created": 0,
    "model": "mock-model",
    "choices": [{
        "index": 0,
        "message": {"role": "assistant", "content": "Hello from the mock LLM."},
        "finish_reason": "stop",
    }],
    "usage": {"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10},
}).encode("utf-8")

_counter = [0]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802 - http.server naming
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        _counter[0] += 1
        sys.stderr.write(
            f"mock-stub request #{_counter[0]}: POST {self.path}\n"
        )
        sys.stderr.flush()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        self.wfile.write(BODY)

    def log_message(self, *args):  # silence default access log
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18123
    server = HTTPServer(("127.0.0.1", port), Handler)
    sys.stderr.write(f"mock-stub listening on 127.0.0.1:{port}\n")
    sys.stderr.flush()
    server.serve_forever()
