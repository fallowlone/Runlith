#!/usr/bin/env python3
"""TASK-0152: minimal local stub of the Anthropic Messages API.

Binds to 127.0.0.1 on a random high port (caller reads the chosen port from
stdout: "PORT=<n>"). Implements just enough of POST /v1/messages (and any
other path the CLI happens to hit) to answer with a fixed marker string,
optionally as an SSE stream. Every incoming request is logged to a SANITIZED
JSONL file: method, path, header NAMES only, and a boolean for whether each
auth-carrying header's VALUE equals the expected dummy placeholder. Header
VALUES themselves are never written to the log or printed to stdout/stderr,
with two narrow, deliberate exceptions used by R8 (round 2): the RAW value
of `anthropic-beta` and `anthropic-version` is recorded, because beta/API
version flags are not secrets or account identifiers -- they are static
capability negotiation strings the CLI ships in its own binary, the same
for every user. No other header value is ever logged, and this exception
list is not user-extensible from outside this file.

Stdlib only (http.server), per TASK-0152 scope constraints.
"""

import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MARKER = os.environ.get("TASK0152_MARKER", "TASK0152_EVIDENCE_OK")
DUMMY_CREDENTIAL = os.environ.get(
    "TASK0152_DUMMY_CREDENTIAL", "runlith-task0152-probe-invalid-credential"
)
LOG_PATH = os.environ["TASK0152_STUB_LOG"]  # required: sanitized request log

_log_lock = threading.Lock()


def _sanitized_log(entry: dict) -> None:
    with _log_lock:
        with open(LOG_PATH, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry, sort_keys=True) + "\n")


AUTH_HEADER_NAMES = ("x-api-key", "authorization")
# Non-secret, non-identifier headers whose raw value is safe to log (see
# module docstring). Anything not in this exact tuple never has its value
# recorded, regardless of what a caller asks for.
SAFE_VALUE_HEADER_NAMES = ("anthropic-beta", "anthropic-version")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # silence default stderr logging
        pass

    def _record(self):
        header_names = sorted({k.lower() for k in self.headers.keys()})
        auth_value_matches = {}
        for name in AUTH_HEADER_NAMES:
            value = self.headers.get(name)
            if value is not None:
                # Only a boolean is retained -- never the header value.
                stripped = value[7:] if value.lower().startswith("bearer ") else value
                auth_value_matches[name] = stripped == DUMMY_CREDENTIAL
        safe_values = {}
        for name in SAFE_VALUE_HEADER_NAMES:
            value = self.headers.get(name)
            if value is not None:
                safe_values[name] = value
        credential_header = None
        if self.headers.get("x-api-key") is not None:
            credential_header = "x-api-key"
        elif self.headers.get("authorization") is not None:
            credential_header = "authorization"
        _record_len = int(self.headers.get("Content-Length", 0) or 0)
        body_raw = self.rfile.read(_record_len) if _record_len else b""
        stream_requested = False
        try:
            if body_raw:
                parsed = json.loads(body_raw)
                stream_requested = bool(parsed.get("stream"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            pass
        _sanitized_log(
            {
                "method": self.command,
                "path": self.path,
                "header_names": header_names,
                "auth_header_value_equals_dummy": auth_value_matches,
                "credential_header": credential_header,
                "safe_header_values": safe_values,
                "stream_requested": stream_requested,
            }
        )
        return stream_requested

    def _messages_response_body(self) -> bytes:
        return json.dumps(
            {
                "id": "msg_task0152stub",
                "type": "message",
                "role": "assistant",
                "model": "claude-stub-0",
                "content": [{"type": "text", "text": MARKER}],
                "stop_reason": "end_turn",
                "stop_sequence": None,
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }
        ).encode("utf-8")

    def _write_sse(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()

        def event(name, data):
            payload = f"event: {name}\ndata: {json.dumps(data)}\n\n"
            self.wfile.write(payload.encode("utf-8"))

        event(
            "message_start",
            {
                "type": "message_start",
                "message": {
                    "id": "msg_task0152stub",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-stub-0",
                    "content": [],
                    "stop_reason": None,
                    "stop_sequence": None,
                    "usage": {"input_tokens": 1, "output_tokens": 0},
                },
            },
        )
        event(
            "content_block_start",
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
        )
        event(
            "content_block_delta",
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": MARKER},
            },
        )
        event("content_block_stop", {"type": "content_block_stop", "index": 0})
        event(
            "message_delta",
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                "usage": {"output_tokens": 1},
            },
        )
        event("message_stop", {"type": "message_stop"})

    def _write_json(self):
        body = self._messages_response_body()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        stream_requested = self._record()
        accept = (self.headers.get("Accept") or "").lower()
        if stream_requested or "text/event-stream" in accept:
            self._write_sse()
        else:
            self._write_json()

    def do_GET(self):
        self._record()
        self._write_json()

    def do_HEAD(self):
        self._record()
        self.send_response(200)
        self.end_headers()


def main():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    port = server.server_address[1]
    print(f"PORT={port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
