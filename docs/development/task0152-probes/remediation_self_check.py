#!/usr/bin/env python3
"""Synthetic checks of actual probe code; no sockets, DNS, child or Keychain.

Run: python3 -I -B docs/development/task0152-probes/remediation_self_check.py
Each failure exits nonzero. Timed doubles prove model handler termination,
not Darwin/TLS/provider behavior. No real credential-shaped inputs.
"""
import contextlib
import getpass
import importlib.util
import io
import json
from pathlib import Path
import socket
import sys
import threading
import time
import types
import warnings
from email.message import Message
from unittest.mock import patch


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


proxy = load('task0152_proxy', 'owner_proxy_probe.py')
provision = load('task0152_provision', 'provision_keychain_item.py')


class SocketDouble:
    def __init__(self):
        self.closed = False
        self.cancelled = threading.Event()

    def shutdown(self, how):
        self.cancelled.set()

    def close(self):
        self.closed = True

    def settimeout(self, timeout):
        pass

    def connect(self, address):
        pass

    def do_handshake(self):
        pass


def state(mode='api-key'):
    s = proxy.SharedState(mode, 'PUBLIC_SYNTHETIC_MARKER', 'PUBLIC_UPSTREAM_DOUBLE', proxy.Counters())
    s.upstream_address = (socket.AF_INET, ('192.0.2.1', 443))
    return s


def server(s):
    v = object.__new__(proxy.ProxyServer)
    v.state = s
    return v


def handler(s, mode='api-key', length='2', extra=()):
    h = object.__new__(proxy.ProxyHandler)
    h.server = server(s)
    h.connection = SocketDouble()
    h._io_lock = threading.Lock()
    h._cancelled = False
    h._upstream_socket = None
    h._connection_end = time.monotonic() + proxy.CONNECTION_DEADLINE_SECONDS
    h.command, h.path = 'POST', '/v1/messages'
    h.headers = Message()
    h.headers['Content-Length'] = length
    h.headers[s.cred_header] = s.placeholder
    for k, v in extra:
        h.headers[k] = v
    h.rfile, h.wfile = io.BytesIO(b'{}'), io.BytesIO()
    h.codes = []
    h.send_response = h.codes.append
    h.send_header = lambda *args: None
    h.end_headers = lambda: None
    return h


def dispatch(h):
    # Admission-only checks replace timer scheduling; lifecycle checks below
    # retain actual timers and execute process_request_thread through cleanup.
    with patch.object(h, '_start_deadline'):
        h._dispatch()


def slots():
    s = state()
    servers = [server(s), server(s)]
    held = [SocketDouble() for _ in range(proxy.MAX_CONCURRENT_CONNECTIONS)]
    for i, sock in enumerate(held):
        assert servers[i % 2].verify_request(sock, ('127.0.0.1', 1))
    for i in range(12):
        v, rejected = servers[i % 2], SocketDouble()
        assert not v.verify_request(rejected, ('127.0.0.1', 1))
        v.shutdown_request(rejected)
        v.close_request(rejected)
        assert s.active_connections == len(held) == proxy.MAX_CONCURRENT_CONNECTIONS
    foreign = SocketDouble()
    assert not servers[0].verify_request(foreign, ('192.0.2.2', 1))
    servers[0].shutdown_request(foreign)
    servers[0].close_request(held.pop())
    admitted = SocketDouble()
    assert servers[1].verify_request(admitted, ('::1', 1, 0, 0))
    held.append(admitted)
    for sock in held:
        servers[1].close_request(sock)
        servers[0].close_request(sock)
    assert s.active_connections == 0
    # Even close exceptions release only the slot owned by that request.
    sock = SocketDouble()
    assert servers[0].verify_request(sock, ('127.0.0.1', 1))
    with patch.object(sock, 'close', side_effect=OSError):
        try:
            servers[0].close_request(sock)
        except OSError:
            pass
    servers[0].close_request(sock)
    assert s.active_connections == 0


def request_reservations():
    s = state()
    s.counters.forwarded = proxy.MAX_FORWARDED_REQUESTS - 1
    count = proxy.MAX_CONCURRENT_CONNECTIONS
    start = threading.Barrier(count + 1)
    reached, release, lock = threading.Event(), threading.Event(), threading.Lock()
    pending, outcomes, failures = [], [], []

    def record(outcome):
        with lock:
            outcomes.append(outcome)
            if len(outcomes) == count:
                reached.set()

    def forward(*args):
        pending.append(args)
        record(200)
        assert release.wait(1), 'test forward was not released'

    def run():
        try:
            h = handler(s)
            h._forward = forward
            h.send_response = record
            start.wait(1)
            dispatch(h)
        except BaseException as error:
            failures.append(type(error).__name__)

    workers = [threading.Thread(target=run) for _ in range(count)]
    for worker in workers:
        worker.start()
    try:
        start.wait(1)
        assert reached.wait(.4), 'not all concurrent admissions completed'
        assert len(pending) == 1 and s.counters.forwarded == proxy.MAX_FORWARDED_REQUESTS
        assert outcomes.count(503) == count - 1
    finally:
        release.set()
        for worker in workers:
            worker.join(1)
    assert not failures and not any(worker.is_alive() for worker in workers)
    assert s.counters.forwarded == proxy.MAX_FORWARDED_REQUESTS  # completion did not recharge


def body_length():
    h = handler(state(), length='50')
    h._forward = lambda *args: (_ for _ in ()).throw(AssertionError('short EOF forwarded'))
    dispatch(h)
    assert h.codes == [400] and h.server.state.counters.forwarded == 0


class ResponseDouble(io.BytesIO):
    status = 200

    def getheaders(self):
        return [('Content-Type', 'application/json')]


class ConnectionDouble:
    def __init__(self, response):
        self.response = response
        self.sock = None
        self.closed = False
        self.requests = []

    def request(self, method, path, body, headers):
        self.requests.append((method, path, body, headers))

    def getresponse(self):
        return self.response

    def close(self):
        self.closed = True
        self.response.close()
        if self.sock:
            self.sock.close()


@contextlib.contextmanager
def forward_doubles(sock, conn):
    # Both old opener and current direct connection are doubled; no live I/O.
    opener = types.SimpleNamespace(open=lambda *args, **kwargs: conn.response)
    ctx = types.SimpleNamespace(wrap_socket=lambda *args, **kwargs: sock)
    with patch.object(proxy, '_OPENER', opener), patch.object(proxy, '_TLS_CONTEXT', ctx, create=True), \
            patch.object(proxy.socket, 'socket', return_value=sock), \
            patch.object(proxy.http.client, 'HTTPSConnection', return_value=conn):
        yield


def headers_and_response_limit():
    for mode in ('api-key', 'oauth'):
        s = state(mode)
        marker = s.placeholder
        h = handler(s, extra=[(marker, 'x'), ('anthropic-beta', 'public-beta')])
        sock, conn = SocketDouble(), ConnectionDouble(ResponseDouble(b'{}'))
        output = io.StringIO()
        with forward_doubles(sock, conn), contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            h._forward(h.path, b'{}')
            print(json.dumps(s.counters.snapshot()))
        assert marker not in output.getvalue(), 'raw header name in diagnostics'
        assert h.codes == [200]
        assert len(conn.requests) == 1
        headers = conn.requests[0][3]
        assert marker not in headers and headers['anthropic-beta'] == 'public-beta'
        assert headers[s.cred_header].endswith(s.real_credential)
        assert headers['Host'] == proxy.UPSTREAM_HOST
        assert conn.closed and conn.response.closed and sock.closed
    h = handler(state())
    # Patch the configured byte bound to avoid allocating megabytes.
    with patch.object(proxy, 'MAX_RESPONSE_BYTES', 4, create=True):
        try:
            h._relay_response(ResponseDouble(b'12345'), 200, {})
        except ValueError:
            pass
        else:
            raise AssertionError('over-limit response relayed')
    assert h.wfile.getvalue() == b''
    h = handler(state())
    with patch.object(proxy, 'MAX_RESPONSE_BYTES', 4, create=True):
        try:
            h._relay_response(ResponseDouble(b'12345'), 200, {'Content-Type': 'text/event-stream'})
        except ValueError:
            pass
        else:
            raise AssertionError('over-limit SSE relayed')
    assert h.wfile.getvalue() == b'1234'


def forward_failure_and_redirect():
    for status in (302, 401):
        s = state()
        h = handler(s)
        sock = SocketDouble()
        response = ResponseDouble(b'{}')
        response.status = status
        conn = ConnectionDouble(response)
        with forward_doubles(sock, conn):
            dispatch(h)
        assert h.codes == [status] and len(conn.requests) == 1
        assert s.counters.forwarded == 1 and conn.auto_open == 0
    s = state()
    s.counters.forwarded = proxy.MAX_FORWARDED_REQUESTS - 1
    h = handler(s)
    sock, conn = SocketDouble(), ConnectionDouble(ResponseDouble(b'{}'))
    sock.connect = lambda addr: (_ for _ in ()).throw(OSError(s.placeholder))
    output = io.StringIO()
    with forward_doubles(sock, conn), contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
        dispatch(h)
        print(json.dumps(s.counters.snapshot()))
    assert s.placeholder not in output.getvalue()
    assert s.counters.forwarded == proxy.MAX_FORWARDED_REQUESTS
    h = handler(s)
    dispatch(h)
    assert h.codes == [503]  # failure did not restore retry authority


def stalled_lifecycle(stage, cancel=False):
    s, entered = state(), threading.Event()
    h = handler(s)
    sock = SocketDouble()

    def stall(*args):
        entered.set()
        assert sock.cancelled.wait(1), 'test cleanup did not cancel double'
        raise OSError('PUBLIC_SYNTHETIC_MARKER')

    response = ResponseDouble(b'{}')
    conn = ConnectionDouble(response)
    if stage == 'connect':
        sock.connect = stall
    elif stage == 'tls':
        sock.do_handshake = stall
    elif stage == 'send':
        conn.request = stall
    elif stage == 'headers':
        conn.getresponse = stall
    elif stage == 'read':
        response.read = stall
    elif stage == 'sse':
        response.getheaders = lambda: [('Content-Type', 'text/event-stream')]
        response.read = stall
    elif stage == 'consumer':
        h.wfile.write = stall
    elif stage == 'trickle':
        response.getheaders = lambda: [('Content-Type', 'text/event-stream')]
        response.read = lambda n: b'x' if not sock.cancelled.is_set() else b''
        h.wfile.write = lambda data: entered.set()
    v = h.server
    assert v.verify_request(h.connection, ('127.0.0.1', 1))
    v.finish_request = lambda *args: h.handle_one_request()
    failures = []

    def run():
        try:
            v.process_request_thread(h.connection, ('127.0.0.1', 1))
        except BaseException as exc:
            failures.append(type(exc).__name__)

    output = io.StringIO()
    with forward_doubles(sock, conn), \
            patch.object(proxy.http.server.BaseHTTPRequestHandler, 'handle_one_request', lambda self: self._dispatch()), \
            patch.object(proxy, 'FORWARD_DEADLINE_SECONDS', .05, create=True), \
            patch.object(proxy, 'CONNECTION_DEADLINE_SECONDS', .15, create=True), \
            contextlib.redirect_stderr(output):
        worker = threading.Thread(target=run)
        worker.start()
        try:
            assert entered.wait(.3), 'double never reached'
            if cancel:
                s.cancel_connections()
            worker.join(.4)
            assert not worker.is_alive(), 'handler survived terminal deadline/cancellation'
            assert not failures and s.active_connections == 0 and not s.handlers
            assert sock.closed and h.connection.closed and conn.closed and response.closed
            assert s.counters.forwarded == 1, 'failure charged twice or restored reservation'
            assert s.placeholder not in output.getvalue()
        finally:
            sock.shutdown(socket.SHUT_RDWR)
            worker.join(1)


def hidden_input():
    class NoRead(io.StringIO):
        def readline(self, *args):
            raise AssertionError('fallback stdin was read')

    out = io.StringIO()
    with patch.object(getpass.os, 'open', side_effect=OSError), \
            patch.object(sys, 'stdin', NoRead()), patch.object(sys, 'stderr', out):
        try:
            provision._read_secret_getpass()
        except SystemExit as error:
            assert error.code == 2

        else:
            raise AssertionError('unhidden input accepted')
    def warning(*args):
        warnings.warn('synthetic echo-control failure', getpass.GetPassWarning)
        raise AssertionError('continued after warning')
    with patch.object(getpass, 'getpass', side_effect=warning), contextlib.redirect_stderr(out):
        try:
            provision._read_secret_getpass()
        except SystemExit as error:
            assert error.code == 2
    with patch.object(getpass, 'getpass', side_effect=['public-test', 'public-test']):
        assert provision._read_secret_getpass() == 'public-test'
    with patch.object(sys, 'stdin', io.StringIO('public-test\npublic-test\n')):
        assert provision._read_secret_stdin_for_test() == 'public-test'
    # Exercise the actual test-path admission; never invoke security/clipboard.
    with patch.object(sys, 'argv', ['provision', 'api-key', '--stdin-for-test']), \
            patch.object(provision, '_item_exists', side_effect=AssertionError('gate bypass')), \
            contextlib.redirect_stderr(out):
        try:
            provision.main()
        except SystemExit as error:
            assert error.code == 2


def dry_failures_propagate():
    # No live dry probes: every I/O helper is doubled. Printed FAIL must
    # make the extended result false, which _real_main turns into exit 1.
    s = state()
    opener = types.SimpleNamespace(open=lambda *args, **kwargs: (_ for _ in ()).throw(
        proxy.urllib.error.HTTPError('http://synthetic', 403, '', {}, None)))
    proc = types.SimpleNamespace(returncode=2, stdout='', stderr='')
    output = io.StringIO()
    with patch.object(proxy, 'run_self_check', return_value=(False, 200)), \
            patch.object(proxy, '_OPENER', opener), \
            patch.object(proxy, '_raw_http_request', side_effect=[400, 400, 413]), \
            patch.object(proxy, '_check_whole_request_deadline', return_value=True), \
            patch.object(proxy, '_check_connection_limit', return_value=True), \
            patch.object(proxy.subprocess, 'run', return_value=proc), contextlib.redirect_stdout(output):
        assert proxy._extended_dry_checks(s, 1) is False
    assert 'check_wrong_placeholder_401=fail' in output.getvalue()


def main():
    checks = [(check.__name__, check) for check in
              (slots, request_reservations, body_length, headers_and_response_limit,
               forward_failure_and_redirect, hidden_input, dry_failures_propagate)]
    checks += [(f'deadline_{stage}', lambda stage=stage: stalled_lifecycle(stage)) for stage in
               ('connect', 'tls', 'send', 'headers', 'read', 'sse', 'consumer', 'trickle')]
    checks += [('attempt_cancellation', lambda: stalled_lifecycle('read', cancel=True))]
    failed = 0
    # Tripwires prohibit accidental network/process/Keychain execution.
    with patch.object(socket, 'create_connection', side_effect=AssertionError('live socket')), \
            patch.object(socket, 'getaddrinfo', side_effect=AssertionError('live DNS')), \
            patch.object(proxy.subprocess, 'run', side_effect=AssertionError('live process')):
        for name, check in checks:
            try:
                check()
                print(f'check_{name}=PASS')
            except Exception as error:
                failed += 1
                print(f'check_{name}=FAIL ({type(error).__name__}: {error})')
    return bool(failed)


if __name__ == '__main__':
    sys.exit(main())
