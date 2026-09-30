#!/usr/bin/env python3
"""Bounded actual CLI capture. Dummy credential; Seatbelt; no upstream socket.

Usage: python3 -I -B capture_profile.py /exact/installed/claude
Artifacts go to a new private temporary directory, never an owner HOME.
"""
import hashlib
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import signal
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PROFILE_ID = 'claude-2.1.283-proposal-api-key-v1'
PROMPT = 'Reply with exactly TASK0152_PROFILE_OK and nothing else.'
ARGV = ['-p', '--safe-mode', '--tools', '', '--disallowedTools', 'mcp__*',
        '--output-format', 'json', PROMPT]
LIMITS = {'seconds': 45, 'requests': 8, 'request_bytes': 2 * 1024 * 1024,
          'total_bytes': 8 * 1024 * 1024, 'output_bytes': 65536}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    if not sys.flags.isolated or len(sys.argv) != 2 or sys.platform != 'darwin':
        raise SystemExit('requires isolated Python, macOS and one executable path')
    executable = Path(sys.argv[1]).resolve(strict=True)
    scripts = Path(__file__).resolve().parent
    root = Path(tempfile.mkdtemp(prefix='task0152-profile-')).resolve()
    started_utc = datetime.now(timezone.utc).isoformat()
    runtime, workspace = root / 'runtime', root / 'workspace'
    for name in ('home', 'config', 'data', 'state', 'cache', 'scratch'):
        (runtime / name).mkdir(parents=True, mode=0o700)
    workspace.mkdir(mode=0o700)
    dummy = secrets.token_urlsafe(32)  # memory only; never persisted
    env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(runtime / 'home'),
           'USER': 'task0152', 'PWD': str(workspace),
           'CLAUDE_CONFIG_DIR': str(runtime / 'config'),
           'CLAUDE_CODE_TMPDIR': str(runtime / 'scratch'),
           'LANG': 'en_US.UTF-8', 'LC_ALL': 'en_US.UTF-8',
           'ANTHROPIC_API_KEY': dummy}
    env.update({f'XDG_{name.upper()}_HOME': str(runtime / name)
                for name in ('config', 'data', 'state', 'cache')})
    env.update({name: str(runtime / 'scratch') for name in ('TMPDIR', 'TMP', 'TEMP')})
    records, failures, total = [], [], [0]
    identifiers = {}

    def sanitize(value):
        value = value.replace(dummy, '<credential:dummy-api-key>')
        value = value.replace(str(root), '<CAPTURE_ROOT>')
        value = re.sub(r'(\\"device_id\\":\\")[0-9a-f]{64}(\\")',
                       r'\1<EPHEMERAL_DEVICE_ID>\2', value)
        # UUIDs originate exclusively in this fresh ephemeral CLI identity.
        def identifier(match):
            token = match.group()
            if token not in identifiers:
                identifiers[token] = f'<EPHEMERAL_ID_{len(identifiers) + 1}>'
            return identifiers[token]
        return re.sub(r'\b[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\b', identifier, value)

    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def log_message(self, *args):
            pass

        def handle_one_request(self):
            self.connection.settimeout(3)
            super().handle_one_request()
            self.close_connection = True

        def send_error(self, code, message=None, explain=None):
            failures.append('http_parser_refusal')
            self.send_response_only(code)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def record(self):
            lengths = self.headers.get_all('Content-Length') or ['0']
            if len(records) >= LIMITS['requests'] or len(lengths) != 1 or not lengths[0].isdecimal():
                raise ValueError('capture_limit_or_framing')
            length = int(lengths[0])
            total[0] += length
            if length > LIMITS['request_bytes'] or total[0] > LIMITS['total_bytes']:
                raise ValueError('capture_byte_limit')
            raw = self.rfile.read(length)
            if len(raw) != length or self.headers.get('Transfer-Encoding'):
                raise ValueError('capture_framing')
            headers = []
            for name, value in self.headers.raw_items():
                if name.lower() in ('authorization', 'x-api-key'):
                    value = '<credential:dummy-api-key>' if value == dummy else '<credential:unexpected>'
                elif name.lower() == 'host':
                    value = '<LOCAL_ENDPOINT>'
                headers.append([sanitize(name.lower()), sanitize(value)])
            entry = {'source': 'actual_cli_request', 'method': self.command,
                     'target': sanitize(self.path), 'headers': headers,
                     'body_utf8': sanitize(raw.decode('utf-8', errors='strict')),
                     'wire_body_bytes': length}
            records.append(entry)
            with (root / 'sanitized-capture.jsonl').open('a') as output:
                output.write(json.dumps(entry, ensure_ascii=True) + '\n')
            return raw

        def do_HEAD(self):
            self.record()
            self.send_response(200)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def do_POST(self):
            raw = self.record()
            if self.path != '/v1/messages?beta=true':
                self.send_error(403)
                return
            body = json.loads(raw)
            if not body.get('stream'):
                self.send_error(400)
                return
            message = {'id': 'msg_task0152_synthetic', 'type': 'message', 'role': 'assistant',
                       'model': body['model'], 'content': [], 'stop_reason': None,
                       'stop_sequence': None, 'usage': {'input_tokens': 1, 'output_tokens': 0}}
            events = [('message_start', {'type': 'message_start', 'message': message}),
                      ('content_block_start', {'type': 'content_block_start', 'index': 0,
                                               'content_block': {'type': 'text', 'text': ''}}),
                      ('content_block_delta', {'type': 'content_block_delta', 'index': 0,
                                               'delta': {'type': 'text_delta', 'text': 'TASK0152_PROFILE_OK'}}),
                      ('content_block_stop', {'type': 'content_block_stop', 'index': 0}),
                      ('message_delta', {'type': 'message_delta',
                                         'delta': {'stop_reason': 'end_turn', 'stop_sequence': None},
                                         'usage': {'output_tokens': 1}}),
                      ('message_stop', {'type': 'message_stop'})]
            # Synthetic responses are a separate artifact, not provider receipts.
            with (root / 'synthetic-responses.jsonl').open('a') as output:
                output.write(json.dumps({'source': 'synthetic_endpoint', 'events': events}) + '\n')
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Connection', 'close')
            self.end_headers()
            for name, data in events:
                self.wfile.write(f'event: {name}\ndata: {json.dumps(data)}\n\n'.encode())
                self.wfile.flush()

    class Server(HTTPServer):
        allow_reuse_address = False

        def handle_error(self, *args):
            failures.append('capture_handler_failure')

    with Server(('127.0.0.1', 0), Handler) as server:
        port = server.server_address[1]
        env['ANTHROPIC_BASE_URL'] = f'http://127.0.0.1:{port}'
        profile = root / 'diagnostic.sb'
        subprocess.run(['/bin/bash', str(scripts / 'gen_profile.sh'), '--workspace', str(workspace),
                        '--claude-exec', str(executable), '--runtime-root', str(runtime),
                        '--network', f'local:{port}', '--out', str(profile)],
                       env={'PATH': '/usr/bin:/bin:/usr/sbin:/sbin'}, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        sb = profile.read_text()
        launch = ['/usr/bin/sandbox-exec', '-p', sb, str(executable)]
        version = subprocess.run(launch + ['--version'], cwd=workspace, env=env,
                                 capture_output=True, timeout=10, check=True).stdout.decode().strip()
        worker = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': .05})
        worker.start()
        started = time.monotonic()
        proc = subprocess.Popen(launch + ARGV, cwd=workspace, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        output = bytearray()
        timed_out = False
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(proc.stdout, selectors.EVENT_READ)
                while selector.get_map():
                    if time.monotonic() - started > LIMITS['seconds'] or failures:
                        timed_out = True
                        break
                    for key, _ in selector.select(.1):
                        chunk = os.read(key.fd, 4096)
                        if not chunk:
                            selector.unregister(key.fd)
                        else:
                            output.extend(chunk)
                            if len(output) > LIMITS['output_bytes']:
                                raise ValueError('output_limit')
        finally:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait(timeout=5)
            proc.stdout.close()
            server.shutdown()
            worker.join(5)
        result = {'profile_id': PROFILE_ID, 'version': version, 'executable_sha256': digest(executable),
                  'started_utc': started_utc,
                  'executable_identity': f'claude/versions/{executable.name}', 'argv': ARGV,
                  'environment_names': sorted(env), 'credential_class': 'dummy-api-key',
                  'limits': LIMITS, 'exit': proc.returncode, 'timed_out': timed_out,
                  'marker_present': b'TASK0152_PROFILE_OK' in output, 'requests': len(records),
                  'failures': failures, 'seconds': round(time.monotonic() - started, 3),
                  'isolation': sanitize(sb), 'harness_sha256': digest(Path(__file__)),
                  'generator_sha256': digest(scripts / 'gen_profile.sh'),
                  'sanitization': ['credential header values replaced before persistence',
                                   'temporary root replaced before persistence',
                                   'fresh CLI UUIDs replaced by correlated categories',
                                   'device identifier replaced by fixed category',
                                   'Host replaced by local endpoint category',
                                   'body text retained; no fields removed; wire length kept separately'],
                  'artifacts': {name: digest(root / name) for name in
                                ('sanitized-capture.jsonl', 'synthetic-responses.jsonl') if (root / name).exists()}}
        result['isolation'] = result['isolation'].replace(str(executable), '<CLI_EXECUTABLE>')
        (root / 'run.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({'artifact_directory': str(root), 'exit': proc.returncode,
                          'requests': len(records), 'marker_present': result['marker_present']}))
        return bool(failures or timed_out or proc.returncode or not result['marker_present'])


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        print(f'capture_failed:{type(error).__name__}', file=sys.stderr)
        sys.exit(1)
