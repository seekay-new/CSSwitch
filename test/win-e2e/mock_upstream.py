"""Standalone mock Anthropic upstream for the Windows end-to-end check.

Records every request it receives to a JSONL file (path, model, body bytes,
auth headers present) and replies with a minimal non-streaming Messages body.
Stdlib only, so it runs with whatever Python 3 is on the machine.
"""

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: mock_upstream.py <port> <jsonl-log-path>", file=sys.stderr)
        return 2
    port = int(sys.argv[1])
    log_path = sys.argv[2]
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):  # keep stdout clean
            pass

        def _record(self, body: bytes):
            try:
                parsed = json.loads(body or b"{}")
            except Exception:
                parsed = {}
            entry = {
                "method": self.command,
                "path": self.path,
                "model": parsed.get("model"),
                "stream": bool(parsed.get("stream")),
                "has_x_api_key": bool(self.headers.get("x-api-key")),
                "has_authorization": bool(self.headers.get("authorization")),
                "max_tokens": parsed.get("max_tokens"),
            }
            with lock, open(log_path, "a", encoding="utf-8") as handle:
                handle.write(json.dumps(entry, ensure_ascii=False) + "\n")

        def do_GET(self):
            self._record(b"")
            body = json.dumps(
                {"data": [{"id": "win-e2e-model", "type": "model"}], "has_more": False}
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            raw = self.rfile.read(length) if length else b""
            self._record(raw)
            if self.path.endswith("/messages"):
                body = json.dumps(
                    {
                        "id": "msg_win_e2e",
                        "type": "message",
                        "role": "assistant",
                        "model": "win-e2e-model",
                        "content": [{"type": "text", "text": "ok-from-mock-upstream"}],
                        "stop_reason": "end_turn",
                        "stop_sequence": None,
                        "usage": {"input_tokens": 1, "output_tokens": 2},
                    }
                ).encode()
            else:
                body = json.dumps({"ok": True}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"mock upstream listening on 127.0.0.1:{port}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
