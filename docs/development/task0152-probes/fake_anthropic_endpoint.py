#!/usr/bin/env python3
"""Fake Anthropic Messages API endpoint for TASK-0152 credential-input probes.

Purpose
-------
Serves just enough of the Anthropic Messages API surface for the installed
Claude Code CLI to complete one non-tool turn against a loopback address, so
the probe matrix in ``run_credential_input_probes.sh`` never contacts a real
Anthropic endpoint and never needs a real credential.

Privacy contract (TASK-0152 hard rule)
---------------------------------------
This process NEVER writes a full header value or a request/response body to
disk. For every request it appends one JSON line to the log file containing
only:

- the HTTP method;
- the request path with the query string stripped;
- the sorted list of request header NAMES (never values);
- for the three auth-relevant headers (``x-api-key``, ``authorization``,
  ``anthropic-beta``) a classification of what the header carried:
  - ``absent`` - header not sent;
  - ``dummy:<marker-name>`` - header value contained one of the known
    TASK-0152 dummy markers;
  - for ``anthropic-beta`` only: the literal value is recorded, because beta
    feature-flag names are not secret;
  - ``non-dummy-value-present`` - the header carried something else. This is
    the STOP/ESCALATE condition from the task envelope: it means a
    non-dummy, potentially real, credential reached this process. The
    endpoint records the fact (never the value) and also writes a sentinel
    file (``<log>.SECRET_LEAK_DETECTED``) next to the log so the driving
    shell script aborts loudly.

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

DUMMY_MARKERS = {
    "api_key": "RUNLITH_T0152_DUMMY_API_KEY_NOT_A_SECRET",
    "bearer": "RUNLITH_T0152_DUMMY_BEARER_NOT_A_SECRET",
    "oauth": "RUNLITH_T0152_DUMMY_OAUTH_NOT_A_SECRET",
}

AUTH_HEADERS = ("x-api-key", "authorization", "anthropic-beta")

_log_lock = threading.Lock()


def classify_auth_value(header_name: str, value: str | None) -> str:
    if value is None:
        return "absent"
    if header_name == "anthropic-beta":
        # Beta feature-flag token names are not secret; record as-is.
        return value.strip()
    for marker_name, marker_value in DUMMY_MARKERS.items():
        if marker_value in value:
            return f"dummy:{marker_name}"
    return "non-dummy-value-present"


def sse_body(text: str, model: str) -> bytes:
    events = []

    def ev(name: str, data: dict) -> None:
        events.append(f"event: {name}\ndata: {json.dumps(data)}\n\n")

    ev(
        "message_start",
        {
            "type": "message_start",
            "message": {
                "id": "msg_probe_task0152",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": model,
                "stop_reason": None,
                "stop_sequence": None,
                "usage": {"input_tokens": 1, "output_tokens": 0},
            },
        },
    )
    ev(
        "content_block_start",
        {
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""},
        },
    )
    ev(
        "content_block_delta",
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": text},
        },
    )
    ev("content_block_stop", {"type": "content_block_stop", "index": 0})
    ev(
        "message_delta",
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": None},
            "usage": {"output_tokens": 5},
        },
    )
    ev("message_stop", {"type": "message_stop"})
    return "".join(events).encode("utf-8")


def json_body(text: str, model: str) -> bytes:
    return json.dumps(
        {
            "id": "msg_probe_task0152",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 1, "output_tokens": 5},
        }
    ).encode("utf-8")


class Handler(BaseHTTPRequestHandler):
    server_version = "Task0152FakeAnthropic/1"
    marker_text = "TASK0152_PROBE_OK"
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
            if any(
                record.get("auth", {}).get(h) == "non-dummy-value-present"
                for h in ("x-api-key", "authorization")
            ):
                sentinel = self.log_path + ".SECRET_LEAK_DETECTED"
                with open(sentinel, "a", encoding="utf-8") as handle:
                    handle.write(line + "\n")

    def _record(self, method: str) -> dict:
        path = urlsplit(self.path).path
        header_names = sorted({name.lower() for name in self.headers.keys()})
        auth = {}
        for header_name in AUTH_HEADERS:
            raw = self.headers.get(header_name)
            auth[header_name] = classify_auth_value(header_name, raw)
        record = {
            "ts": time.time(),
            "method": method,
            "path": path,
            "header_names": header_names,
            "auth": auth,
        }
        self._write_log(record)
        return record

    def _read_body_json(self) -> dict:
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw = self.rfile.read(length) if length else b""
        # Body is parsed in memory only to decide streaming vs. non-streaming
        # and to select a model name for the reply; it is never persisted.
        try:
            return json.loads(raw.decode("utf-8")) if raw else {}
        except (ValueError, UnicodeDecodeError):
            return {}

    def do_POST(self):  # noqa: N802 - stdlib handler naming
        record = self._record("POST")
        path = record["path"]
        if path in ("/v1/messages", "/v1/messages/"):
            payload = self._read_body_json()
            model = str(payload.get("model", "probe-model"))
            stream = bool(payload.get("stream", False))
            if stream:
                body = sse_body(self.marker_text, model)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            else:
                body = json_body(self.marker_text, model)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            return
        if path in ("/v1/messages/count_tokens", "/v1/messages/count_tokens/"):
            self._read_body_json()
            body = json.dumps({"input_tokens": 1}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self._read_body_json()
        self._not_found()

    def do_GET(self):  # noqa: N802 - stdlib handler naming
        record = self._record("GET")
        path = record["path"]
        if path in ("/v1/models", "/v1/models/"):
            body = json.dumps({"data": [], "has_more": False}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self._not_found()

    def _not_found(self) -> None:
        body = json.dumps(
            {
                "type": "error",
                "error": {
                    "type": "not_found_error",
                    "message": "task0152 probe endpoint: no route for this path",
                },
            }
        ).encode("utf-8")
        self.send_response(404)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--log", required=True, help="path to append JSONL request records to")
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
