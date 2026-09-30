#!/usr/bin/env python3
"""Actual stdlib parser regression; synthetic streams, no network or processes."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import socket
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('checks', Path(__file__).with_name('remediation_self_check.py'))
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)


def main():
    marker = b'PUBLIC_SYNTHETIC_MARKER'
    cases = [(marker + b' /v1/messages HTTP/1.1\r\nHost: synthetic\r\n\r\n', 403),
             (b'POST ' + marker + b' HTTP/invalid\r\n\r\n', 400)]
    with patch.object(socket, 'socket', side_effect=AssertionError('live socket')), \
            patch.object(socket, 'getaddrinfo', side_effect=AssertionError('live DNS')), \
            patch.object(checks.proxy.subprocess, 'run', side_effect=AssertionError('live process')):
        for payload, status in cases:
            state = checks.state()
            handler = checks.handler(state)
            handler.rfile, handler.wfile = io.BytesIO(payload), io.BytesIO()
            handler.client_address = ('127.0.0.1', 1)
            for name in ('send_response', 'send_header', 'end_headers'):
                delattr(handler, name)
            assert handler.server.verify_request(handler.connection, handler.client_address)
            handler.server.finish_request = lambda *args: handler.handle_one_request()
            output = io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                handler.server.process_request_thread(handler.connection, handler.client_address)
            response = handler.wfile.getvalue()
            assert response.startswith(f'HTTP/1.1 {status} '.encode()), response
            assert marker not in response and marker.decode() not in output.getvalue()
            assert json.loads(response.split(b'\r\n\r\n', 1)[1]) == {
                'type': 'error', 'error': {'type': 'local_proxy_rejection'}}
            assert state.counters.forwarded == 0
            assert state.counters.local_403 == (1 if status == 403 else 0)
            assert state.active_connections == 0 and not state.handlers and handler.connection.closed
    print('RR-N1: PASS (unknown method 403; malformed parser 400; fixed response; cleanup)')


if __name__ == '__main__':
    main()
