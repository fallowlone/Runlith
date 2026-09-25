#!/usr/bin/env python3
"""Tiny logging HTTP CONNECT proxy for TASK-0152 egress inventory (P7).

Set ``HTTPS_PROXY``/``HTTP_PROXY`` to this process's address and the CLI's
outbound HTTPS attempts arrive here as ``CONNECT host:port`` requests. This
proxy:

- logs only ``host:port`` for every CONNECT target (one JSON line per
  attempt, never any payload, header value, or tunneled byte);
- ALWAYS refuses to tunnel: it replies ``403 Forbidden`` and closes the
  connection. No bytes are ever relayed to any upstream host, so no real
  network egress occurs through this proxy regardless of what the CLI tried
  to reach.
- logs plain (non-CONNECT) proxied requests the same way, recording only
  the request line's method and target authority/host, never the full URL
  path or any header value.

Stdlib only. Python 3.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

_log_lock = threading.Lock()


class Handler(BaseHTTPRequestHandler):
    server_version = "Task0152LoggingConnectProxy/1"
    log_path = None  # set by main()

    def log_message(self, fmt, *args):  # noqa: D401 - silence default stderr logging
        return

    def _write_log(self, record: dict) -> None:
        if not self.log_path:
            return
        line = json.dumps(record, sort_keys=True)
        with _log_lock:
            with open(self.log_path, "a", encoding="utf-8") as handle:
                handle.write(line + "\n")

    def do_CONNECT(self):  # noqa: N802 - stdlib handler naming
        # self.path is "host:port" for a CONNECT request; no payload is read.
        target = self.path
        self._write_log({"ts": time.time(), "method": "CONNECT", "target": target})
        self.send_response(403, "Tunneling refused by task0152 logging proxy")
        self.send_header("Content-Length", "0")
        self.end_headers()
        try:
            self.connection.close()
        except OSError:
            pass

    def _handle_plain(self, method: str) -> None:
        host = urlsplit(self.path).netloc or self.headers.get("Host", "")
        self._write_log({"ts": time.time(), "method": method, "target": host})
        body = b"task0152 logging proxy: egress refused"
        self.send_response(403)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):  # noqa: N802
        self._handle_plain("GET")

    def do_POST(self):  # noqa: N802
        self._handle_plain("POST")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--log", required=True, help="path to append JSONL CONNECT records to")
    parser.add_argument(
        "--ready-file",
        default=None,
        help="if set, this file is created once the server is listening",
    )
    args = parser.parse_args()

    Handler.log_path = args.log
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    if args.ready_file:
        with open(args.ready_file, "w", encoding="utf-8") as handle:
            handle.write(f"{args.host}:{args.port}\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
