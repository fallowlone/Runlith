#!/usr/bin/env python3
"""TASK-0152 round 2 -- Option B (trusted-host authenticated proxy) owner-run
probe. NOT run by the implementer agent (per its constraints: no login, no
Keychain read, no `security` invocation). The owner runs this manually.

Design (matches the ADR's Option B: the sandbox never holds the real
credential; the trusted host does):

  1. This process runs UNSANDBOXED on the host and binds a tiny local HTTP
     proxy to 127.0.0.1 on a random port.
  2. It generates a random per-run PLACEHOLDER (`secrets.token_urlsafe`)
     and gives ONLY that placeholder to the confined child, in the same
     env-var class the real credential would use
     (api-key mode: ANTHROPIC_API_KEY=<placeholder>;
      oauth mode: CLAUDE_CODE_OAUTH_TOKEN=<placeholder>).
  3. It reads the REAL credential from a Keychain generic-password item the
     owner created themselves, via `/usr/bin/security find-generic-password
     ... -w` (the value is returned on stdout; the subprocess argv never
     contains it). The real credential exists only in this process's own
     memory and is used only to build one outbound HTTPS request header --
     it is never printed, never logged, never written to any file, and
     never reaches the confined child in any form.
  4. Any request the confined child sends that does NOT carry the exact
     placeholder in its credential header is rejected locally with 401 --
     the request never leaves this host.
  5. A request that DOES carry the placeholder, on the one allowed path
     (`POST /v1/messages`, any query string), is forwarded to the real
     Anthropic API with the credential header rewritten to the real value.
     Every other path/method is rejected locally with 403 -- also never
     leaves this host in any form the child could observe (the *proxy*
     itself of course must reach the real internet for the one allowed
     forward; the CONFINED CHILD's own network scope is localhost-only,
     enforced by the Seatbelt profile in step 6). The forward is made
     through a dedicated opener (`_OPENER`, round 3 hardening) that (a)
     never follows a redirect -- a 3xx from upstream is relayed to the
     child exactly as received rather than this process re-sending the
     just-rewritten credential header to whatever host a `Location` might
     name -- and (b) ignores ambient `http_proxy`/`https_proxy` env vars,
     so the credential-bearing request cannot be silently routed through a
     proxy the owner never asked this script to use. Default TLS
     verification is unchanged. A defensive assertion also confirms the
     request's own host is exactly `api.anthropic.com` immediately before
     the real credential is attached to it.
  6. The confined child itself is launched by this script, under the
     production-shaped Seatbelt profile from gen_profile.sh, but with the
     network rule replaced by "outbound to this proxy's local port only"
     (same technique R6/R7/R8 used) and a fresh, empty HOME/
     CLAUDE_CONFIG_DIR under this run's own temp directory (same pattern as
     R5 and owner_positive_probe.sh) -- no host `~/.claude` is granted.

Env-var hygiene for the confined child: this script never spawns an `env`
process with the credential (or even the placeholder) as an argv element.
`subprocess.Popen(argv, env=explicit_dict)` passes the environment via the
child's envp at exec time, which is a separate mechanism from argv --
nothing is ever placed in any process's command line. This achieves the
same "no secret in any argv" property `owner_positive_probe.sh` achieves
with the bash `compgen -e`/`unset`/`export`/`exec` sequence, through
Python's native subprocess mechanism instead of replicating that shell
trick pointlessly. (In any case the confined child here never receives the
real credential at all -- only the random placeholder -- so this class of
exposure cannot occur for the child launch regardless of technique; the
real credential's only appearance anywhere is as one HTTP header value on
one outbound TLS connection this process makes itself.)

Caveat this script cannot fully honor: Python strings are immutable, so
unlike a bash `unset`, there is no way to guarantee the real credential's
bytes are overwritten in process memory after use. The reference is kept
in the narrowest possible scope (one local variable, one function) and
dropped as soon as it is no longer needed; this is a documented best
effort, not a guarantee, and is stated here rather than glossed over.

Prints ONLY: CLI version, mode, child exit status, marker present/absent,
a classified failure reason, and integer counts (local_401, local_403 and
its rejected paths, forwarded, and the list of upstream HTTP status
codes). Never header values, bodies, account/org/request IDs, or the
credential/placeholder.

Stdlib only.
"""

import argparse
import hmac
import http.server
import json
import os
import secrets
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

VALID_MODES = ("api-key", "oauth")
CRED_VAR = {"api-key": "ANTHROPIC_API_KEY", "oauth": "CLAUDE_CODE_OAUTH_TOKEN"}
# Credential header per mode, as measured by TASK-0152 round 2 R8 against
# the local stub (docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md):
# ANTHROPIC_API_KEY -> `x-api-key`; CLAUDE_CODE_OAUTH_TOKEN -> `authorization:
# Bearer <token>`. Not guessed.
CRED_HEADER = {"api-key": "x-api-key", "oauth": "authorization"}
UPSTREAM_HOST = "api.anthropic.com"
UPSTREAM_URL = f"https://{UPSTREAM_HOST}"
ALLOWED_FORWARD_PATH = "/v1/messages"
HELLO_PATH = "/api/hello"

# Headers stripped from both directions of the proxy. `content-length` is
# stripped on the response side only when the body is being re-buffered
# with a freshly computed length, or dropped entirely for a streamed SSE
# response (see _forward). `host`/`x-api-key`/`authorization` are handled
# explicitly, never blindly copied.
HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
}

MARKER = "TASK0152_PROXY_PROBE_OK"
PROMPT = f"Do not use any tools. Reply with exactly {MARKER} and nothing else."

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
GEN_PROFILE = os.path.join(SCRIPT_DIR, "gen_profile.sh")


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """Never follows a redirect. `redirect_request` returning `None` makes
    `http_error_30x` return the original response object as-is instead of
    issuing a second request -- so a 3xx from upstream is relayed to the
    child exactly as received (and counted as one forwarded/upstream-status
    entry, same as any other status), and this process never re-sends the
    freshly rewritten credential header to whatever host a `Location`
    might name. Only one HTTPS connection is ever made per forwarded
    request, always to the URL this script built itself
    (`UPSTREAM_URL + path`, asserted below)."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


# Built once, reused for every forward and for the self-check request.
# `urllib.request.ProxyHandler({})` -- an EMPTY mapping, not the default
# environment-reading constructor -- means this opener ignores any
# ambient `http_proxy`/`https_proxy`/`HTTPS_PROXY`/`NO_PROXY` the host
# environment might set, so the credential-bearing request cannot be
# silently routed through a proxy the owner never asked this script to
# use. No custom SSL context is installed, so `build_opener`'s default
# `HTTPSHandler` (which uses `ssl.create_default_context()`: full chain
# and hostname verification) is unchanged.
_OPENER = urllib.request.build_opener(_NoRedirect(), urllib.request.ProxyHandler({}))


class Counters:
    def __init__(self):
        self.lock = threading.Lock()
        self.local_401 = 0
        self.local_403 = 0
        self.local_403_paths = []
        self.forwarded = 0
        self.upstream_status = []

    def snapshot(self):
        with self.lock:
            return {
                "local_401": self.local_401,
                "local_403": self.local_403,
                "local_403_paths": sorted(set(self.local_403_paths)),
                "forwarded": self.forwarded,
                "upstream_status": list(self.upstream_status),
            }


class ProxyServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, addr, handler_cls, mode, placeholder, real_credential, counters):
        super().__init__(addr, handler_cls)
        self.mode = mode
        self.placeholder = placeholder
        self.real_credential = real_credential  # None until the real run needs it
        self.cred_header = CRED_HEADER[mode]
        self.counters = counters


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "task0152-option-b-proxy"

    def log_message(self, fmt, *args):  # silence default stderr logging
        pass

    # -- local responses -----------------------------------------------
    def _send_local(self, code):
        body = json.dumps({"type": "error", "error": {"type": "local_proxy_rejection"}}).encode(
            "utf-8"
        )
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _credential_ok(self):
        value = self.headers.get(self.server.cred_header)
        if value is None:
            return False
        stripped = value[7:] if value.lower().startswith("bearer ") else value
        # Constant-time comparison (hmac.compare_digest) rather than `==`,
        # so response timing cannot be used to guess the placeholder byte
        # by byte. The placeholder is a per-run random token, not a
        # long-lived secret, but the comparison is hardened anyway since
        # it is the sole gate on whether a request gets locally rejected
        # or forwarded with the real credential.
        return hmac.compare_digest(
            stripped.encode("utf-8", errors="replace"),
            self.server.placeholder.encode("utf-8"),
        )

    # -- dispatch ---------------------------------------------------------
    def do_HEAD(self):
        self._dispatch(read_body=False)

    def do_GET(self):
        self._dispatch(read_body=False)

    def do_POST(self):
        self._dispatch(read_body=True)

    def _dispatch(self, read_body):
        base_path = self.path.split("?", 1)[0]
        length = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(length) if (read_body and length) else b""

        if self.command in ("HEAD", "GET") and base_path == HELLO_PATH:
            self.send_response(200)
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return

        if not (self.command == "POST" and base_path == ALLOWED_FORWARD_PATH):
            with self.server.counters.lock:
                self.server.counters.local_403 += 1
                self.server.counters.local_403_paths.append(base_path)
            self._send_local(403)
            return

        if not self._credential_ok():
            with self.server.counters.lock:
                self.server.counters.local_401 += 1
            self._send_local(401)
            return

        if self.server.real_credential is None:
            # Dry mode / self-check-only: credential matched, but this
            # server was never given a real credential to forward with.
            # Refuse locally rather than attempting an upstream call with
            # nothing to send -- this branch is only reachable in
            # --dry-self-check, and the self-check itself always sends a
            # WRONG placeholder, so it never reaches here in practice.
            self._send_local(403)
            return

        self._forward(self.path, body)

    # -- forwarding ---------------------------------------------------------
    def _forward(self, path, body):
        headers = {}
        for name, value in self.headers.items():
            lname = name.lower()
            if lname in HOP_BY_HOP or lname == "host":
                continue
            if lname in ("x-api-key", "authorization"):
                continue  # rewritten below, never blindly copied through
            headers[name] = value
        # Forward anthropic-beta / anthropic-version exactly as the CLI
        # sent them (TASK-0152 R8 measured the CLI already sends the
        # correct beta flag set per credential mode, including the
        # OAuth-specific `oauth-2025-04-20` flag in oauth mode on its own
        # -- nothing here needs to add or guess a flag).
        if self.server.mode == "api-key":
            headers["x-api-key"] = self.server.real_credential
        else:
            headers["authorization"] = f"Bearer {self.server.real_credential}"
        headers["Host"] = UPSTREAM_HOST

        url = UPSTREAM_URL + path
        # Defense in depth: the path allowlist in `_dispatch` already
        # restricts `path` to exactly `ALLOWED_FORWARD_PATH` (plus any
        # query string) before `_forward` is ever called, so this should
        # be unreachable -- but the credential-header rewrite above never
        # trusts that invariant blindly either. The real credential is
        # attached to a request only after this assertion confirms the
        # request's own host is exactly the real API, never a value that
        # could route it, or a redirect from it, anywhere else (redirects
        # are separately never followed at all -- see `_NoRedirect`).
        if urllib.parse.urlsplit(url).hostname != UPSTREAM_HOST:
            self.send_response(502)
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return
        req = urllib.request.Request(url, data=body if body else None, headers=headers, method="POST")
        try:
            # Default TLS verification (no custom SSLContext -- urllib's
            # default `HTTPSHandler` in `_OPENER` uses
            # `ssl.create_default_context()`, full chain and hostname
            # verification). `_OPENER` also never follows a redirect and
            # never honours an ambient proxy env var -- see its
            # construction above.
            with _OPENER.open(req, timeout=60) as resp:
                self._relay_response(resp, resp.status, dict(resp.getheaders()))
        except urllib.error.HTTPError as error:
            self._relay_response(error, error.code, dict(error.headers or {}))
        except Exception:
            with self.server.counters.lock:
                self.server.counters.forwarded += 1
                self.server.counters.upstream_status.append(0)
            self.send_response(502)
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()

    def _relay_response(self, resp_like, status, resp_headers):
        with self.server.counters.lock:
            self.server.counters.forwarded += 1
            self.server.counters.upstream_status.append(status)
        content_type = ""
        for k, v in resp_headers.items():
            if k.lower() == "content-type":
                content_type = v
                break
        is_sse = "text/event-stream" in content_type.lower()

        self.send_response(status)
        for k, v in resp_headers.items():
            if k.lower() in HOP_BY_HOP or k.lower() == "content-length":
                continue
            self.send_header(k, v)
        if is_sse:
            # Stream unbuffered: no Content-Length (unknown/streamed),
            # connection close signals end-of-body to the client, matching
            # stub_server.py's SSE handling used in R6/R7/R8.
            self.send_header("Connection", "close")
            self.end_headers()
            try:
                while True:
                    chunk = resp_like.read(1)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
        else:
            data = resp_like.read()
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Connection", "close")
            self.end_headers()
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass


def read_keychain_credential(service, account):
    """Reads a generic-password item on the HOST, outside any sandbox. The
    value is returned on stdout by `-w`; the subprocess argv never contains
    it, and it is never printed, logged, or written to disk by this
    function or any caller."""
    try:
        completed = subprocess.run(
            ["/usr/bin/security", "find-generic-password", "-s", service, "-a", account, "-w"],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except Exception as error:  # noqa: BLE001 -- report class only, never details that could echo input
        print(f"refusing: failed to invoke security ({type(error).__name__})", file=sys.stderr)
        sys.exit(2)
    if completed.returncode != 0:
        print(
            f"refusing: no Keychain item found for service={service!r} account={account!r}",
            file=sys.stderr,
        )
        sys.exit(2)
    value = completed.stdout.strip()
    if not value:
        print("refusing: Keychain item is empty", file=sys.stderr)
        sys.exit(2)
    return value


def run_self_check(server, port):
    """Sends one request with a WRONG placeholder and asserts: local 401,
    zero upstream forwards. Never touches the real credential (the check
    fails at the placeholder comparison, before forwarding would even be
    considered) and never contacts upstream -- safe to run in dry mode."""
    before = server.counters.snapshot()["forwarded"]
    wrong_value = "wrong-" + secrets.token_urlsafe(16)
    if server.cred_header == "authorization":
        header_value = f"Bearer {wrong_value}"
    else:
        header_value = wrong_value
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{ALLOWED_FORWARD_PATH}",
        data=b'{"model":"x","messages":[],"max_tokens":1}',
        headers={server.cred_header: header_value, "Content-Type": "application/json"},
        method="POST",
    )
    try:
        _OPENER.open(req, timeout=10)
        status = 200  # would be unexpected -- the check below still catches it
    except urllib.error.HTTPError as error:
        status = error.code
    after = server.counters.snapshot()["forwarded"]
    ok = status == 401 and after == before
    return ok, status


def find_claude_exec():
    resolved = shutil.which("claude")
    if not resolved:
        print("refusing: claude executable not found on PATH", file=sys.stderr)
        sys.exit(2)
    return os.path.realpath(resolved)


def cli_version(claude_exec):
    try:
        out = subprocess.run([claude_exec, "--version"], capture_output=True, text=True, timeout=15)
        return out.stdout.strip() or "unknown"
    except Exception:
        return "unknown"


def launch_confined_child(mode, placeholder, port, claude_exec):
    """Launches the confined child with ONLY the placeholder (never the
    real credential), under the production-shaped profile with a
    localhost-only network rule, a fresh empty HOME/CLAUDE_CONFIG_DIR, and
    no host `~/.claude` support-read at all (same pattern as R5 and
    owner_positive_probe.sh). Returns (exit_code, marker_present,
    stdout_text_bounded)."""
    workdir = tempfile.mkdtemp(prefix="task0152-proxy-")
    try:
        workspace = os.path.join(workdir, "workspace")
        runtime_root = os.path.join(workdir, "runtime")
        fresh_home = os.path.join(workdir, "home")
        for sub in (
            workspace,
            runtime_root,
            os.path.join(runtime_root, "config"),
            os.path.join(runtime_root, "data"),
            os.path.join(runtime_root, "state"),
            os.path.join(runtime_root, "cache"),
            os.path.join(runtime_root, "scratch"),
            fresh_home,
        ):
            os.makedirs(sub, exist_ok=True)
        os.chmod(runtime_root, 0o700)
        os.chmod(fresh_home, 0o700)

        profile_path = os.path.join(workdir, "production-shaped.sb")
        gen = subprocess.run(
            [
                "bash",
                GEN_PROFILE,
                "--workspace",
                workspace,
                "--claude-exec",
                claude_exec,
                "--runtime-root",
                runtime_root,
                "--network",
                f"local:{port}",
                "--out",
                profile_path,
            ],
            capture_output=True,
            text=True,
        )
        if gen.returncode != 0:
            print(gen.stdout, file=sys.stderr)
            print(gen.stderr, file=sys.stderr)
            return None, False, "harness_error: profile generation failed"

        with open(profile_path, "r", encoding="utf-8") as fh:
            profile_text = fh.read()

        # Explicit, closed environment -- passed via `env=` (envp at exec
        # time), never via any process's argv. Only the PLACEHOLDER is
        # present; the real credential is never constructed into this
        # dict at all.
        child_env = {
            "PATH": f"/usr/bin:/bin:/usr/sbin:/sbin:{os.path.dirname(claude_exec)}",
            "HOME": fresh_home,
            "USER": os.environ.get("USER", "task0152"),
            "PWD": workspace,
            "XDG_CONFIG_HOME": os.path.join(runtime_root, "config"),
            "XDG_DATA_HOME": os.path.join(runtime_root, "data"),
            "XDG_STATE_HOME": os.path.join(runtime_root, "state"),
            "XDG_CACHE_HOME": os.path.join(runtime_root, "cache"),
            "CLAUDE_CODE_TMPDIR": os.path.join(runtime_root, "scratch"),
            "TMPDIR": os.path.join(runtime_root, "scratch"),
            "TMP": os.path.join(runtime_root, "scratch"),
            "TEMP": os.path.join(runtime_root, "scratch"),
            "CLAUDE_CONFIG_DIR": os.path.join(fresh_home, ".claude-config"),
            "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{port}",
            CRED_VAR[mode]: placeholder,
        }

        argv = [
            "/usr/bin/sandbox-exec",
            "-p",
            profile_text,
            claude_exec,
            "-p",
            "--safe-mode",
            "--output-format",
            "json",
            PROMPT,
        ]
        try:
            completed = subprocess.run(
                argv,
                cwd=workspace,
                env=child_env,
                capture_output=True,
                text=True,
                timeout=120,
            )
        except subprocess.TimeoutExpired:
            return None, False, "timeout"

        marker_present = MARKER in completed.stdout
        return completed.returncode, marker_present, completed.stdout
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


def classify(exit_code, marker_present, stdout_text):
    if marker_present:
        return "success"
    if exit_code is None:
        return "harness_error"
    lowered = (stdout_text or "").lower()
    if "not logged in" in lowered or "/login" in lowered:
        return "not logged in"
    if "401" in lowered or "authentication_error" in lowered or "invalid" in lowered:
        return "auth rejected"
    return f"unclassified (exit {exit_code})"


def main():
    parser = argparse.ArgumentParser(
        description="TASK-0152 Option B owner-run proxy probe. Never prints the credential."
    )
    parser.add_argument("mode", choices=VALID_MODES, help="api-key or oauth")
    parser.add_argument("--service", help="Keychain -s service name (required unless --dry-self-check)")
    parser.add_argument("--account", default=os.environ.get("USER", ""), help="Keychain -a account name")
    parser.add_argument(
        "--dry-self-check",
        action="store_true",
        help="Run argument validation and the negative self-check only. "
        "Never calls `security`, never contacts upstream, never launches the confined child.",
    )
    args = parser.parse_args()

    if args.mode not in VALID_MODES:
        print(f"refusing: unknown mode {args.mode!r}", file=sys.stderr)
        sys.exit(2)

    if not args.dry_self_check and not args.service:
        print("refusing: --service is required outside --dry-self-check", file=sys.stderr)
        sys.exit(2)

    placeholder = secrets.token_urlsafe(32)
    counters = Counters()

    if args.dry_self_check:
        real_credential = None  # never touches Keychain
    else:
        real_credential = None  # set after the self-check passes, below

    server = ProxyServer(("127.0.0.1", 0), ProxyHandler, args.mode, placeholder, real_credential, counters)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        ok, status = run_self_check(server, port)
        print(f"self_check_pass={ok}")
        print(f"self_check_status={status}")
        if not ok:
            print("refusing: negative self-check failed -- aborting before any real work", file=sys.stderr)
            sys.exit(1)

        if args.dry_self_check:
            counts = counters.snapshot()
            print(f"mode={args.mode}")
            print(f"dry_run=true")
            print(f"local_401={counts['local_401']}")
            print(f"local_403={counts['local_403']}")
            print(f"forwarded={counts['forwarded']}")
            return

        claude_exec = find_claude_exec()
        version = cli_version(claude_exec)

        # Only now, after the self-check has proven the local rejection
        # path works, is the real credential read.
        real_credential = read_keychain_credential(args.service, args.account)
        server.real_credential = real_credential
        try:
            exit_code, marker_present, stdout_text = launch_confined_child(
                args.mode, placeholder, port, claude_exec
            )
        finally:
            # Best-effort only -- see module docstring: Python strings
            # cannot be securely wiped. Drop every reference this process
            # holds as soon as the child has run.
            real_credential = None
            server.real_credential = None

        cls = classify(exit_code, marker_present, stdout_text)
        counts = counters.snapshot()

        print(f"cli_version={version}")
        print(f"mode={args.mode}")
        print(f"child_exit={exit_code}")
        print(f"marker_present={marker_present}")
        print(f"failure_class={cls}")
        print(f"local_401={counts['local_401']}")
        print(f"local_403={counts['local_403']}")
        print(f"local_403_paths={counts['local_403_paths']}")
        print(f"forwarded={counts['forwarded']}")
        print(f"upstream_status_codes={counts['upstream_status']}")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
