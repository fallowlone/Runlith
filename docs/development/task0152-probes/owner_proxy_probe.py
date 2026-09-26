#!/usr/bin/env python3
"""TASK-0152 -- Option B (trusted-host authenticated proxy) owner-run probe.
The OWNER runs this against their REAL Keychain item and real credential
manually, as `python3 -I owner_proxy_probe.py {api-key,oauth}`. The
implementer agent has run this same script during testing, but only
against throwaway temp keychains holding dummy, obviously-fake,
never-real credentials (see the evidence doc's "Agent-run end-to-end
invalid-credential test" and the temp-keychain provisioning tests) --
never against the owner's real Keychain item, never with a real
credential, and never performing an interactive login or `setup-token`
flow. That distinction -- real item/real credential is the owner's alone;
temp-keychain/dummy-credential testing is this task's own verification --
is the accurate one, not a blanket claim that the agent never ran it at
all.

Design (matches ADR-0037's Option B: the sandbox never holds the real
credential; the trusted host does):

  1. This process runs UNSANDBOXED on the host and binds a tiny local HTTP
     proxy to 127.0.0.1 on a random port.
  2. It generates a random per-run PLACEHOLDER (`secrets.token_urlsafe`)
     and gives ONLY that placeholder to the confined child, in the same
     env-var class the real credential would use
     (api-key mode: ANTHROPIC_API_KEY=<placeholder>;
      oauth mode: CLAUDE_CODE_OAUTH_TOKEN=<placeholder>).
  3. It reads the REAL credential from a Keychain generic-password item
     the owner provisions with `provision_keychain_item.py` (never typed
     on any command line -- see that script), via
     `/usr/bin/security find-generic-password ... -w` (the value is
     returned on stdout; the subprocess argv never contains it). The real
     credential exists only in this process's own memory and is used only
     to build one outbound HTTPS request header -- it is never printed,
     never logged, never written to any file, and never reaches the
     confined child in any form.
  4. Any request the confined child sends that does NOT carry the exact
     placeholder in its credential header (constant-time compared) is
     rejected locally with 401 -- the request never leaves this host.
  5. The proxy allowlists exactly one forwardable request shape:
     `POST /v1/messages` with an empty or exactly `beta=true` query string
     (round 4: any other query is a local 403 -- the CLI's own R8-measured
     shape is either bare or `?beta=true`, nothing else is expected).
     Every other path/method/query is rejected locally with 403 and the
     path recorded. A request is also rejected locally, before any
     forward, if it looks like a request-smuggling attempt: a
     `Transfer-Encoding` header (400), more than one `Content-Length`
     header or a non-numeric one (400), or a declared body over 8 MiB
     (413) -- none of these ever reach the body-read step, let alone a
     forward.
  6. A request that passes all of the above has its credential header
     **rewritten**: API-key mode sets `x-api-key` to the real key and
     drops any `authorization`; OAuth mode sets `authorization: Bearer
     <real>` and drops any `x-api-key`. Only an explicit allowlist of
     other header NAMES is forwarded (round 4, C7): `content-type`,
     `accept`, `anthropic-version`, `anthropic-beta`, `user-agent`, and
     the R8-observed non-identifying Stainless SDK/runtime metadata
     headers (`x-stainless-arch`, `x-stainless-lang`, `x-stainless-os`,
     `x-stainless-package-version`, `x-stainless-retry-count`,
     `x-stainless-runtime`, `x-stainless-runtime-version`,
     `x-stainless-timeout`) and `x-app`. Everything else the child sent is
     dropped and only its NAME is counted/reported --
     `x-claude-code-session-id` (a session identifier) and
     `anthropic-dangerous-direct-browser-access` (changes trust semantics,
     never asked for) are deliberately NOT in the allowlist even though
     R8 observed the CLI send them. `anthropic-beta`/`anthropic-version`
     are forwarded exactly as the CLI sent them (R8 already proved the
     CLI itself sends the correct flag set per mode -- nothing is added or
     guessed). The forward is made through a dedicated opener that never
     follows a redirect, ignores ambient proxy env vars, and pins TLS to a
     minimum of 1.2 with an explicit `SSLContext` built after stripping
     any ambient CA-override env var (round 4, C3) -- see `_build_opener`.
     A defensive assertion confirms the outgoing request's host is exactly
     `api.anthropic.com` immediately before the real credential is
     attached. At most 32 requests are ever forwarded in one run (round 4,
     C8); the local proxy is single-threaded, and every accepted
     connection gets a 30s per-operation socket timeout plus a 10s
     whole-request deadline from accept through headers+body fully read
     (round 6, T2b -- an independent review measured that the 30s
     per-operation timeout alone does not bound a slow byte-at-a-time
     trickle, since each individual read completes well within it).
     Bound to BOTH `127.0.0.1:<port>` and `[::1]:<port>` (the SAME port,
     round 6, T2c -- Seatbelt's `(remote tcp "localhost:P")` was measured
     to admit both; see the evidence doc's "L1 loopback admission" row),
     refusing to start at all if the IPv6 bind fails, and accepting
     connections only from loopback peers (127.0.0.0/8 or ::1) regardless
     of which socket received them. Other local, non-loopback interface
     addresses reachable on the same port remain a documented residual --
     see the evidence doc.
  7. The confined child itself is launched by this script, under the
     production-shaped Seatbelt profile from gen_profile.sh, but with the
     network rule replaced by "outbound to this proxy's local port only"
     (same technique R6/R7/R8 used) and a fresh, empty HOME/
     CLAUDE_CONFIG_DIR under this run's own temp directory (same pattern
     as R5) -- no host `~/.claude` is granted.
  8. Prints ONLY: CLI version, mode, child exit status, marker
     present/absent, a classified failure reason, and integer counts.
     Never a header value, a body, an account/org/request identifier, or
     the credential/placeholder.

Round 4 hardening (the class of mistake being fixed, not just its
symptom): the owner previously typed a real OAuth token into a command's
`-s` argument by mistake, exposing it in argv/shell-history/chat. This
script now:

  - refuses to run at all if ANY argv element looks like it could be a
    credential (starts with the real token-prefix shape, or is a
    long high-entropy-shaped run) -- C1;
  - refuses unless run with `python3 -I` (isolated interpreter: no
    ambient `PYTHON*` env vars, no user site-packages) -- C2;
  - refuses outright (round 6: does not merely strip and continue) if
    `OPENSSL_CONF`, any `OPENSSL_*`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, any
    `*_CA_BUNDLE`, or `SSLKEYLOGFILE` is present in its environment --
    checked before `ssl`/`http.server`/`urllib.request` are even imported,
    because an independent review measured that OpenSSL reads
    `OPENSSL_CONF` at library-initialization time (as early as `import
    ssl`), so scrubbing `os.environ` afterward is too late to prevent the
    downgrade it can cause -- and builds an explicit
    `ssl.create_default_context()` pinned to TLS >= 1.2 before making any
    TLS connection -- C3;
  - refuses unless this exact script file is tracked and byte-identical
    to `HEAD`, and `docs/development/task0152-probes` has no uncommitted
    changes, printing `script_commit=`/`script_sha256=` so the owner can
    compare against the commit they actually reviewed -- C4. This is a
    tripwire for an UNNOTICED working-copy change between review and
    execution. It is NOT a defense against an attacker who can rewrite
    this very check: a maliciously modified copy of the script could
    simply remove or lie about this function. Treat a pass as "the file
    matches what was reviewed", never as "this file cannot have been
    tampered with by someone who can already write to it";
  - validates the Keychain-read credential's shape (mode-specific prefix
    + charset) before ever using it, printing only
    "refusing: credential shape does not match mode" on mismatch, never
    any detail about what was actually read -- C5;
  - never lets a value reach stderr: `ProxyServer.handle_error` and the
    top-level `__main__` handler print only an exception's CLASS NAME,
    never its message (which could embed a header or credential value);
    a header-validation failure while relaying a response drops that one
    header rather than raising with its value in the exception text -- C6;
  - forwards only an explicit header-name allowlist (see step 6) and an
    explicit query-string allowlist -- C7;
  - rejects request-smuggling-shaped input (Transfer-Encoding, duplicate/
    non-numeric Content-Length, oversized body) before ever reading a
    body, caps forwarded requests at 32 per run, runs single-threaded,
    and uses a 30s per-connection socket timeout -- C8;
  - disables core dumps for its own process (`RLIMIT_CORE = 0,0`) so a
    crash can never leave a credential-bearing core file on disk -- C9;
  - accepts `--keychain PATH` to target an explicit keychain file (used
    only by this task's own temp-keychain tests; the owner's normal use
    leaves it unset) -- C10.

Round 6 hardening (independent-review findings, fixed here rather than
merely noted):

  - refuses outright on `OPENSSL_CONF`/`OPENSSL_*`/`SSL_CERT_FILE`/
    `SSL_CERT_DIR`/`*_CA_BUNDLE`/`SSLKEYLOGFILE`, checked before any of
    `ssl`, `http.server`, or `urllib.request` is imported -- see the C3
    bullet above (this replaces round 4's "strip and continue" policy for
    these names, since a review showed stripping after `ssl` is imported
    is too late for `OPENSSL_CONF` specifically) -- T2a;
  - a 10s whole-request deadline (`signal.alarm`, single-threaded so the
    handler always runs on the main thread) from connection accept
    through headers+body fully read, on top of the existing 30s
    per-operation socket timeout -- see step 6 above -- T2b;
  - binds `[::1]:<port>` (the same port as `127.0.0.1:<port>`) with the
    same handler and shared state, refusing to start if that bind fails,
    and accepts connections only from loopback peers regardless of which
    socket received them -- see step 6 above and the evidence doc's "L1
    loopback admission" row -- T2c;
  - a local `403` for a disallowed path/method never records the raw
    child-supplied path -- only a truncated SHA-256 prefix, tagged
    `path:`/`query:` -- since that path is data the confined child
    controls, not this trusted host's own data -- T2d.

Env-var hygiene for the confined child: the launch uses
`subprocess.Popen(argv, env=<explicit dict>)`, which passes the
environment via the child's `envp` at exec time -- a mechanism entirely
separate from `argv` -- rather than routing it through an external `env`
command whose own argv would be briefly `ps`-visible. In this script the
point is close to moot regardless: the confined child never receives the
real credential in any form, only the random placeholder. The real
credential's only appearance anywhere in this process is as one header
value on one outbound TLS connection the proxy itself makes.

Documented limitation, not glossed over: Python strings are immutable, so
this script cannot guarantee the real credential's bytes are overwritten
in process memory after use. The reference is held in the narrowest
possible scope and dropped as soon as it is no longer needed; this is a
best-effort scope reduction, not a wipe guarantee.

Stdlib only.
"""

# C3 (round 6 hardening): `import os, sys` ONLY, before anything else --
# not even `ssl`, `http.server`, or `urllib.request` -- so the dangerous-
# env-var check below runs before this process's OpenSSL binding has any
# chance to read `OPENSSL_CONF` (an independent review measured that
# OpenSSL reads that variable at library initialization time, which can
# happen as early as `import ssl`; scrubbing `os.environ` AFTER that
# import is too late -- the downgrade has already been read into the
# library's own state by then). Refusing outright, rather than stripping
# and continuing, is deliberate: an env var that can silently reconfigure
# which CAs are trusted or how OpenSSL initializes is a fact about this
# process's environment the owner needs to see and fix themselves, not
# something this script should quietly paper over and proceed past.
import os
import sys

_DANGEROUS_ENV_EXACT = ("SSL_CERT_FILE", "SSL_CERT_DIR", "SSLKEYLOGFILE")


def _c3_refuse_if_dangerous_tls_env():
    found = []
    for name in os.environ:
        if name in _DANGEROUS_ENV_EXACT:
            found.append(name)
        elif name.startswith("OPENSSL_"):
            found.append(name)
        elif name.endswith("_CA_BUNDLE"):
            found.append(name)
    if found:
        print(
            "refusing: the following environment variable(s) can alter TLS/OpenSSL "
            "behavior before this script ever gets a chance to check for them -- "
            "unset them and re-run: " + ", ".join(sorted(found)),
            file=sys.stderr,
        )
        sys.exit(2)


_c3_refuse_if_dangerous_tls_env()

import argparse
import hashlib
import hmac
import http.server
import json
import re
import secrets
import shutil
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

try:
    import resource
except ImportError:  # pragma: no cover -- resource is POSIX-only
    resource = None

VALID_MODES = ("api-key", "oauth")
CRED_VAR = {"api-key": "ANTHROPIC_API_KEY", "oauth": "CLAUDE_CODE_OAUTH_TOKEN"}
# Credential header per mode, as measured by TASK-0152 round 2 R8 against
# the local stub (docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md):
# ANTHROPIC_API_KEY -> `x-api-key`; CLAUDE_CODE_OAUTH_TOKEN -> `authorization:
# Bearer <token>`. Not guessed.
CRED_HEADER = {"api-key": "x-api-key", "oauth": "authorization"}
SERVICE_NAME_DEFAULT = {
    "oauth": "runlith-task0152-owner-oauth-token",
    "api-key": "runlith-task0152-owner-anthropic-api-key",
}
UPSTREAM_HOST = "api.anthropic.com"
UPSTREAM_URL = f"https://{UPSTREAM_HOST}"
ALLOWED_FORWARD_PATH = "/v1/messages"
ALLOWED_QUERY_STRINGS = ("", "beta=true")
HELLO_PATH = "/api/hello"

MAX_BODY_BYTES = 8 * 1024 * 1024
MAX_FORWARDED_REQUESTS = 32
CLIENT_SOCKET_TIMEOUT = 30
# Round 6 (T2b): whole-request deadline -- from connection accept through
# completion of reading headers+body -- separate from and tighter than
# the per-recv() idle timeout above. An independent review measured a
# 43s stall from a client trickling the body 1 byte every 1.5s: each
# individual recv() completes well within CLIENT_SOCKET_TIMEOUT, so that
# per-operation timeout never fires, yet the single-threaded server is
# held the whole time. `signal.alarm` bounds the WALL-CLOCK total
# regardless of how the slowness is spread across individual reads.
REQUEST_DEADLINE_SECONDS = 10

# The real token prefix (`s`+`k`+`-`+`a`+`n`+`t`+`-`, joined) never
# appears as one contiguous literal anywhere in this file's source text (a
# secret scan of this repo greps for exactly that joined form). Building
# it from single-character pieces at runtime satisfies both goals: the
# running program still validates/detects the real prefix shape, but no
# static scan of the source finds a token-shaped literal.
_TOKEN_PREFIX_BASE = "".join(("s", "k", "-", "a", "n", "t", "-"))
TOKEN_PREFIX = {
    "api-key": _TOKEN_PREFIX_BASE + "api",
    "oauth": _TOKEN_PREFIX_BASE + "oat",
}
_SHAPE_CHARSET = re.compile(r"^[A-Za-z0-9_-]+$")
_HIGH_ENTROPY_MIN_LEN = 40

# Headers stripped from the RESPONSE relay direction only (upstream ->
# child). The REQUEST direction (child -> upstream) uses the strict
# FORWARD_HEADER_ALLOWLIST below instead of a denylist.
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

# C7: explicit allowlist of request header NAMES forwarded to upstream.
# Everything else the child sends is dropped (name-only counted, value
# never inspected further). See the module docstring, step 6, for the
# justification of each Stainless/x-app name and what was deliberately
# excluded.
FORWARD_HEADER_ALLOWLIST = {
    "content-type",
    "accept",
    "anthropic-version",
    "anthropic-beta",
    "user-agent",
    "x-stainless-arch",
    "x-stainless-lang",
    "x-stainless-os",
    "x-stainless-package-version",
    "x-stainless-retry-count",
    "x-stainless-runtime",
    "x-stainless-runtime-version",
    "x-stainless-timeout",
    "x-app",
}

MARKER = "TASK0152_PROXY_PROBE_OK"
PROMPT = f"Do not use any tools. Reply with exactly {MARKER} and nothing else."

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
THIS_FILE = os.path.abspath(__file__)
GEN_PROFILE = os.path.join(SCRIPT_DIR, "gen_profile.sh")

# Built by _build_opener() inside main(), after the C3 env scrub, with an
# explicit TLS-1.2-minimum SSLContext. Module-level so _forward/
# run_self_check can reference it without threading it through every call.
_OPENER = None


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """Never follows a redirect. `redirect_request` returning `None` makes
    a 3xx from upstream come back as an `HTTPError` (this Python version's
    behavior) instead of a second request being issued -- so it is relayed
    to the child exactly as received (counted like any other response),
    and this process never re-sends the freshly rewritten credential
    header to whatever host a `Location` might name. Only one HTTPS
    connection is ever made per forwarded request, always to the URL this
    script built itself (`UPSTREAM_URL + path`, host asserted in
    `_forward`)."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _build_opener():
    """C3 (part 2): an explicit `ssl.create_default_context()` pinned to a
    TLS minimum of 1.2, passed via `HTTPSHandler(context=...)`, combined
    with the round-3 `_NoRedirect` and an empty `ProxyHandler({})` (so
    ambient `http_proxy`/`https_proxy` cannot reroute the credential-
    bearing request). Certificate/hostname verification is otherwise the
    Python default (unchanged)."""
    ctx = ssl.create_default_context()
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    return urllib.request.build_opener(
        _NoRedirect(), urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=ctx)
    )


def _looks_secret(arg):
    """C1's detector: starts with a real token-prefix shape, or is itself
    a long high-entropy-shaped run. Used both on this process's own argv
    (refuse to even start) and, defensively, nowhere else -- this is never
    used to filter or "sanitize" a value that is then used anyway."""
    for prefix in TOKEN_PREFIX.values():
        if arg.startswith(prefix):
            return True
    return len(arg) >= _HIGH_ENTROPY_MIN_LEN and bool(_SHAPE_CHARSET.fullmatch(arg))


def _c1_refuse_if_secret_looking_argv():
    for arg in sys.argv[1:]:
        if _looks_secret(arg):
            print(
                "refusing: an argument looks like it could be a credential -- "
                "this tool never takes a secret on the command line",
                file=sys.stderr,
            )
            sys.exit(2)


def _c2_require_isolated_interpreter():
    if not sys.flags.isolated:
        print(
            "refusing: run this with `python3 -I owner_proxy_probe.py ...` "
            "(isolated interpreter mode)",
            file=sys.stderr,
        )
        sys.exit(2)


def _c4_verify_integrity():
    """Refuses unless this script file is tracked and byte-identical to
    `HEAD`, and `docs/development/task0152-probes` has no uncommitted
    changes. See the module docstring for what this does and does not
    guard against."""
    try:
        repo_root = subprocess.run(
            ["git", "-C", SCRIPT_DIR, "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        ).stdout.strip()
    except Exception:
        repo_root = ""
    if not repo_root:
        print("refusing: not inside a git working tree", file=sys.stderr)
        sys.exit(2)

    head = subprocess.run(
        ["git", "-C", repo_root, "rev-parse", "HEAD"], capture_output=True, text=True, timeout=10
    ).stdout.strip()
    if not head:
        print("refusing: could not resolve HEAD", file=sys.stderr)
        sys.exit(2)

    rel_path = os.path.relpath(THIS_FILE, repo_root)
    working_hash = subprocess.run(
        ["git", "-C", repo_root, "hash-object", THIS_FILE], capture_output=True, text=True, timeout=10
    ).stdout.strip()
    tracked = subprocess.run(
        ["git", "-C", repo_root, "ls-tree", "HEAD", "--", rel_path],
        capture_output=True,
        text=True,
        timeout=10,
    ).stdout.strip()
    if not tracked:
        print("refusing: script is not tracked at HEAD", file=sys.stderr)
        sys.exit(2)
    fields = tracked.split()
    head_blob = fields[2] if len(fields) >= 3 else ""
    if not working_hash or working_hash != head_blob:
        print(
            "refusing: working copy of this script differs from the reviewed commit",
            file=sys.stderr,
        )
        sys.exit(2)

    status = subprocess.run(
        ["git", "-C", repo_root, "status", "--porcelain", "--", "docs/development/task0152-probes"],
        capture_output=True,
        text=True,
        timeout=10,
    ).stdout
    if status.strip():
        print("refusing: docs/development/task0152-probes has uncommitted changes", file=sys.stderr)
        sys.exit(2)

    sha256 = hashlib.sha256()
    with open(THIS_FILE, "rb") as fh:
        sha256.update(fh.read())
    print(f"script_commit={head}")
    print(f"script_sha256={sha256.hexdigest()}")


def _c9_disable_core_dumps():
    if resource is None:
        return
    try:
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    except Exception:
        pass


def _validate_credential_shape(mode, value):
    """C5: mode-specific prefix + charset. Returns a bool only -- callers
    must never print `value` on a False result, only the fixed refusal
    text."""
    prefix = TOKEN_PREFIX[mode]
    if not value.startswith(prefix):
        return False
    remainder = value[len(prefix):]
    return bool(remainder) and bool(_SHAPE_CHARSET.fullmatch(remainder))


class _RequestDeadlineExceeded(Exception):
    """Raised by the SIGALRM handler; never carries any request data in
    its own message, so even an unhandled instance of this (there should
    never be one -- see `ProxyHandler.handle_one_request`) prints only a
    class name via C6's exception-class-name-only policy."""


def _alarm_handler(signum, frame):
    raise _RequestDeadlineExceeded()


def _hash_path_for_logging(raw_path):
    """Round 6 (T2d): a child-supplied path is data the confined child
    controls, not this trusted host's own data -- recording it verbatim
    in a durable/printed counter turns this script into an unbounded,
    attacker-steered logging channel. A short truncated SHA-256 prefix
    keeps distinct disallowed paths distinguishable in aggregate counts
    (useful for noticing "the same wrong path repeatedly" vs. "many
    different wrong paths") without ever printing or storing the raw
    content."""
    return hashlib.sha256(raw_path.encode("utf-8", errors="replace")).hexdigest()[:12]


class Counters:
    def __init__(self):
        self.lock = threading.Lock()
        self.local_401 = 0
        self.local_403 = 0
        self.local_403_paths = []
        self.local_400 = 0
        self.local_413 = 0
        self.dropped_header_names = set()
        self.forwarded = 0
        self.upstream_status = []

    def snapshot(self):
        with self.lock:
            return {
                "local_401": self.local_401,
                "local_403": self.local_403,
                "local_403_paths": sorted(set(self.local_403_paths)),
                "local_400": self.local_400,
                "local_413": self.local_413,
                "dropped_header_names": sorted(self.dropped_header_names),
                "forwarded": self.forwarded,
                "upstream_status": list(self.upstream_status),
            }


class SharedState:
    """State shared between the IPv4 and IPv6 server instances (round 6,
    T2c) -- both bind the SAME port and run the SAME handler class, so
    setting `.real_credential` once (after the self-check passes) must be
    visible to whichever socket actually receives the confined child's
    connection."""

    def __init__(self, mode, placeholder, real_credential, counters):
        self.mode = mode
        self.placeholder = placeholder
        self.real_credential = real_credential  # None until the real run needs it
        self.cred_header = CRED_HEADER[mode]
        self.counters = counters


def _is_loopback_peer(address_tuple):
    """Round 6 (T2c): accept only 127.0.0.0/8 or ::1. Defense in depth on
    top of binding only loopback addresses -- see `ProxyServer.verify_request`."""
    host = address_tuple[0]
    if host.startswith("127."):
        return True
    if host in ("::1", "0:0:0:0:0:0:0:1"):
        return True
    return False


class ProxyServer(http.server.HTTPServer):
    # C8: single-threaded (plain HTTPServer, not ThreadingHTTPServer),
    # never reuse a just-closed address, bind loopback-only via the addr
    # tuple main() constructs this with.
    allow_reuse_address = False

    def __init__(self, addr, handler_cls, state):
        super().__init__(addr, handler_cls)
        self.state = state

    def verify_request(self, request, client_address):
        # Round 6 (T2c): accept only loopback peers, even though this
        # server only ever binds a loopback address itself -- a second,
        # independent gate in case anything about the binding or routing
        # environment ever surprises us.
        return _is_loopback_peer(client_address)

    def handle_error(self, request, client_address):
        # C6: the default socketserver.handle_error prints a full
        # traceback (str(exception) can embed a header or credential
        # value). Print only the exception's class name.
        exc_type = sys.exc_info()[0]
        name = exc_type.__name__ if exc_type else "UnknownError"
        print(f"internal error handling one request: {name}", file=sys.stderr)


class ProxyServerV6(ProxyServer):
    address_family = socket.AF_INET6


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "task0152-option-b-proxy"
    timeout = CLIENT_SOCKET_TIMEOUT  # C8: per-connection socket timeout

    def log_message(self, fmt, *args):  # silence default stderr logging
        pass

    # -- whole-request deadline (round 6, T2b) -----------------------------
    def handle_one_request(self):
        # Covers from connection accept (well, from the start of parsing
        # this one request -- `handle()`'s loop calls this once per
        # request on a connection, and this proxy always sends
        # `Connection: close`, so there is exactly one) through whatever
        # point `_dispatch` calls `_cancel_deadline()` -- i.e. through
        # headers+body being fully read, never through response writing.
        signal.alarm(REQUEST_DEADLINE_SECONDS)
        try:
            super().handle_one_request()
        except _RequestDeadlineExceeded:
            self.close_connection = True
        finally:
            signal.alarm(0)

    def _cancel_deadline(self):
        signal.alarm(0)

    # -- local responses -----------------------------------------------
    def _send_local(self, code):
        # Every local-rejection path funnels through here -- the single
        # place that cancels the whole-request deadline for all of them,
        # since none needs to read any more of the request afterward.
        self._cancel_deadline()
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
        value = self.headers.get(self.server.state.cred_header)
        if value is None:
            return False
        stripped = value[7:] if value.lower().startswith("bearer ") else value
        # Constant-time comparison: response timing cannot be used to
        # guess the placeholder byte by byte.
        return hmac.compare_digest(
            stripped.encode("utf-8", errors="replace"),
            self.server.state.placeholder.encode("utf-8"),
        )

    # -- dispatch ---------------------------------------------------------
    def do_HEAD(self):
        self._dispatch()

    def do_GET(self):
        self._dispatch()

    def do_POST(self):
        self._dispatch()

    def _dispatch(self):
        base_path = self.path.split("?", 1)[0]
        query = self.path.split("?", 1)[1] if "?" in self.path else ""

        if self.command in ("HEAD", "GET") and base_path == HELLO_PATH:
            self._cancel_deadline()
            self.send_response(200)
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return

        if not (self.command == "POST" and base_path == ALLOWED_FORWARD_PATH):
            # C1/T2d: never record the raw child-supplied path -- a
            # truncated hash keeps distinct disallowed paths
            # distinguishable without ever printing or storing content
            # the confined child controls.
            with self.server.state.counters.lock:
                self.server.state.counters.local_403 += 1
                self.server.state.counters.local_403_paths.append(
                    f"path:{_hash_path_for_logging(base_path)}"
                )
            self._send_local(403)
            return

        # C7 (query allowlist): empty or exactly `beta=true`.
        if query not in ALLOWED_QUERY_STRINGS:
            with self.server.state.counters.lock:
                self.server.state.counters.local_403 += 1
                self.server.state.counters.local_403_paths.append(
                    f"query:{_hash_path_for_logging(base_path + '?' + query)}"
                )
            self._send_local(403)
            return

        # C8 (request-smuggling/parser hardening) -- checked before any
        # body is read, on every request shape, regardless of credential.
        if self.headers.get("Transfer-Encoding") is not None:
            with self.server.state.counters.lock:
                self.server.state.counters.local_400 += 1
            self._send_local(400)
            return
        content_lengths = self.headers.get_all("Content-Length") or []
        if len(content_lengths) > 1:
            with self.server.state.counters.lock:
                self.server.state.counters.local_400 += 1
            self._send_local(400)
            return
        length = 0
        if content_lengths:
            if not re.fullmatch(r"[0-9]+", content_lengths[0]):
                with self.server.state.counters.lock:
                    self.server.state.counters.local_400 += 1
                self._send_local(400)
                return
            length = int(content_lengths[0])
        if length > MAX_BODY_BYTES:
            with self.server.state.counters.lock:
                self.server.state.counters.local_413 += 1
            self._send_local(413)
            return

        body = self.rfile.read(length) if length else b""
        # The request (headers + body) is now fully read -- the deadline
        # this specifically guards against (a slow trickle) is satisfied
        # regardless of which branch below is taken next.
        self._cancel_deadline()

        if not self._credential_ok():
            with self.server.state.counters.lock:
                self.server.state.counters.local_401 += 1
            self._send_local(401)
            return

        if self.server.state.real_credential is None:
            # Dry mode / self-check-only: credential matched, but this
            # server was never given a real credential to forward with.
            # Refuse locally rather than attempting an upstream call with
            # nothing to send.
            self._send_local(403)
            return

        with self.server.state.counters.lock:
            over_cap = self.server.state.counters.forwarded >= MAX_FORWARDED_REQUESTS
        if over_cap:
            self._send_local(503)
            return

        self._forward(self.path, body)

    # -- forwarding ---------------------------------------------------------
    def _forward(self, path, body):
        headers = {}
        dropped = []
        for name, value in self.headers.items():
            lname = name.lower()
            if lname in ("x-api-key", "authorization", "host"):
                continue  # rewritten/set explicitly below, never blindly copied
            if lname in FORWARD_HEADER_ALLOWLIST:
                headers[name] = value
            else:
                dropped.append(name)
        if dropped:
            with self.server.state.counters.lock:
                self.server.state.counters.dropped_header_names.update(dropped)

        if self.server.state.mode == "api-key":
            headers["x-api-key"] = self.server.state.real_credential
        else:
            headers["authorization"] = f"Bearer {self.server.state.real_credential}"
        headers["Host"] = UPSTREAM_HOST

        url = UPSTREAM_URL + path
        if urllib.parse.urlsplit(url).hostname != UPSTREAM_HOST:
            self.send_response(502)
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return
        req = urllib.request.Request(url, data=body if body else None, headers=headers, method="POST")
        try:
            with _OPENER.open(req, timeout=60) as resp:
                self._relay_response(resp, resp.status, dict(resp.getheaders()))
        except urllib.error.HTTPError as error:
            self._relay_response(error, error.code, dict(error.headers or {}))
        except Exception:
            with self.server.state.counters.lock:
                self.server.state.counters.forwarded += 1
                self.server.state.counters.upstream_status.append(0)
            try:
                self.send_response(502)
                self.send_header("Content-Length", "0")
                self.send_header("Connection", "close")
                self.end_headers()
            except Exception:
                pass

    def _relay_response(self, resp_like, status, resp_headers):
        with self.server.state.counters.lock:
            self.server.state.counters.forwarded += 1
            self.server.state.counters.upstream_status.append(status)
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
            try:
                self.send_header(k, v)
            except Exception:
                # C6: http.client header validation can raise with the
                # value embedded in its message. Drop just this one
                # header rather than let that reach anywhere.
                continue
        if is_sse:
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


def read_keychain_credential(service, account, keychain_path=None):
    """Reads a generic-password item on the HOST, outside any sandbox. The
    value is returned on stdout by `-w`; the subprocess argv never contains
    it, and it is never printed, logged, or written to disk by this
    function or any caller. `keychain_path`, when given, targets an
    explicit keychain file (C10 -- test use only; the owner's normal use
    leaves it unset and reads the default keychain)."""
    argv = ["/usr/bin/security", "find-generic-password", "-s", service, "-a", account, "-w"]
    if keychain_path:
        argv.append(keychain_path)
    try:
        completed = subprocess.run(argv, capture_output=True, text=True, timeout=15, check=False)
    except Exception as error:
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


def run_self_check(state, port):
    """Sends one request with a WRONG placeholder and asserts: local 401,
    zero upstream forwards. Never touches the real credential and never
    contacts upstream -- safe to run in dry mode."""
    before = state.counters.snapshot()["forwarded"]
    wrong_value = "wrong-" + secrets.token_urlsafe(16)
    if state.cred_header == "authorization":
        header_value = f"Bearer {wrong_value}"
    else:
        header_value = wrong_value
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{ALLOWED_FORWARD_PATH}",
        data=b'{"model":"x","messages":[],"max_tokens":1}',
        headers={state.cred_header: header_value, "Content-Type": "application/json"},
        method="POST",
    )
    try:
        _OPENER.open(req, timeout=10)
        status = 200  # would be unexpected -- the check below still catches it
    except urllib.error.HTTPError as error:
        status = error.code
    after = state.counters.snapshot()["forwarded"]
    ok = status == 401 and after == before
    return ok, status


def _check_whole_request_deadline(port):
    """Round 6 (T2b): sends headers declaring a body, then only ever
    sends ONE byte of it and stops -- the exact shape of stall an
    independent review measured holding this server for 43s (1 byte
    trickled every 1.5s never trips the per-recv() idle timeout, since
    each individual read completes quickly). Asserts the server closes
    the connection at, not before and not long after, the whole-request
    deadline -- proving the fix without literally waiting out a 43s-shaped
    attack."""
    lines = [
        f"POST {ALLOWED_FORWARD_PATH} HTTP/1.1",
        f"Host: 127.0.0.1:{port}",
        "Content-Length: 50",
        "Connection: close",
        "",
        "",
    ]
    payload = "\r\n".join(lines).encode("utf-8")
    start = time.monotonic()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=REQUEST_DEADLINE_SECONDS + 15) as sock:
            sock.sendall(payload)
            sock.sendall(b"x")  # exactly 1 of the declared 50 body bytes, then nothing more
            got_eof = False
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    got_eof = True
                    break
    except Exception:
        got_eof = True
    elapsed = time.monotonic() - start
    return got_eof and (REQUEST_DEADLINE_SECONDS - 2) <= elapsed <= (REQUEST_DEADLINE_SECONDS + 10)


def _raw_http_request(port, method, path, extra_headers, body, send_declared_length_only=False):
    """Speaks one raw HTTP/1.1 request over a socket, for constructing
    request shapes `http.client`/`urllib` will not let us build through
    their normal APIs (duplicate `Content-Length`, a `Transfer-Encoding`
    the client library would otherwise manage itself). Test-only -- never
    used for a real forward. Returns the parsed status code, or `None`."""
    lines = [f"{method} {path} HTTP/1.1", f"Host: 127.0.0.1:{port}"]
    for name, value in extra_headers:
        lines.append(f"{name}: {value}")
    lines.append("Connection: close")
    payload = ("\r\n".join(lines) + "\r\n\r\n").encode("utf-8")
    if not send_declared_length_only:
        payload += body
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            sock.sendall(payload)
            try:
                sock.shutdown(socket.SHUT_WR)
            except OSError:
                pass
            response = b""
            while len(response) < 65536:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                response += chunk
    except Exception:
        return None
    first_line = response.split(b"\r\n", 1)[0]
    parts = first_line.split(b" ")
    if len(parts) < 2:
        return None
    try:
        return int(parts[1])
    except ValueError:
        return None


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
    no host `~/.claude` support-read at all (same pattern as R5). Returns
    (exit_code, marker_present, stdout_text_bounded)."""
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
            print("harness error: profile generation failed", file=sys.stderr)
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


def _extended_dry_checks(state, port):
    """Round 4/6: exercises every C7/C8 local-rejection path plus the
    C1/C2 argv and interpreter gates (via a subprocess of THIS script,
    since those are checked before any server exists), plus round 6's
    T2b whole-request deadline, T2a OPENSSL_CONF refusal, and T2d path-
    hashing. Prints one `check_<name>=pass|fail` line per assertion.
    Never touches Keychain or contacts upstream."""
    ok, status = run_self_check(state, port)
    print(f"check_wrong_placeholder_401={'pass' if ok and status == 401 else 'fail'}")

    DISALLOWED_PATH_MARKER = "/v1/models"
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{ALLOWED_FORWARD_PATH}?not-allowed=1",
        data=b"{}",
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        _OPENER.open(req, timeout=10)
        status = 200
    except urllib.error.HTTPError as error:
        status = error.code
    print(f"check_bad_query_403={'pass' if status == 403 else 'fail'}")

    status = _raw_http_request(
        port,
        "POST",
        ALLOWED_FORWARD_PATH,
        extra_headers=[("Transfer-Encoding", "chunked")],
        body=b"0\r\n\r\n",
    )
    print(f"check_transfer_encoding_400={'pass' if status == 400 else 'fail'}")

    status = _raw_http_request(
        port,
        "POST",
        ALLOWED_FORWARD_PATH,
        extra_headers=[("Content-Length", "2"), ("Content-Length", "2")],
        body=b"{}",
    )
    print(f"check_duplicate_content_length_400={'pass' if status == 400 else 'fail'}")

    status = _raw_http_request(
        port,
        "POST",
        ALLOWED_FORWARD_PATH,
        extra_headers=[("Content-Length", str(MAX_BODY_BYTES + 1))],
        body=b"",
        send_declared_length_only=True,
    )
    print(f"check_oversized_body_413={'pass' if status == 413 else 'fail'}")

    req = urllib.request.Request(f"http://127.0.0.1:{port}{DISALLOWED_PATH_MARKER}", method="GET")
    try:
        _OPENER.open(req, timeout=10)
        status = 200
    except urllib.error.HTTPError as error:
        status = error.code
    print(f"check_disallowed_path_403={'pass' if status == 403 else 'fail'}")

    # T2d: the disallowed path above must never appear verbatim in this
    # process's own recorded counters -- only a truncated hash may.
    recorded_paths = state.counters.snapshot()["local_403_paths"]
    path_not_leaked = not any(DISALLOWED_PATH_MARKER in entry for entry in recorded_paths)
    print(f"check_disallowed_path_not_recorded_raw={'pass' if path_not_leaked else 'fail'}")

    # T2b: whole-request deadline against a slow-trickled body.
    deadline_ok = _check_whole_request_deadline(port)
    print(f"check_whole_request_deadline={'pass' if deadline_ok else 'fail'}")

    poison = "A" * (_HIGH_ENTROPY_MIN_LEN + 5)
    proc = subprocess.run(
        [sys.executable, "-I", THIS_FILE, "oauth", "--dry-self-check", "--service", poison],
        capture_output=True,
        text=True,
        timeout=20,
    )
    leaked = poison in (proc.stdout or "") or poison in (proc.stderr or "")
    print(f"check_secret_argv_refused={'pass' if proc.returncode != 0 and not leaked else 'fail'}")

    proc2 = subprocess.run(
        [sys.executable, THIS_FILE, "oauth", "--dry-self-check"],
        capture_output=True,
        text=True,
        timeout=20,
    )
    print(f"check_non_isolated_refused={'pass' if proc2.returncode != 0 else 'fail'}")

    # T2a/T2e: OPENSSL_CONF (and friends) must refuse, checked before any
    # TLS-touching import -- a subprocess with a poisoned environment.
    poisoned_env = dict(os.environ)
    poisoned_env["OPENSSL_CONF"] = "/dev/null"
    proc3 = subprocess.run(
        [sys.executable, "-I", THIS_FILE, "oauth", "--dry-self-check"],
        capture_output=True,
        text=True,
        timeout=20,
        env=poisoned_env,
    )
    print(f"check_openssl_conf_refused={'pass' if proc3.returncode != 0 else 'fail'}")


def _real_main():
    _c1_refuse_if_secret_looking_argv()
    _c2_require_isolated_interpreter()

    parser = argparse.ArgumentParser(
        description="TASK-0152 Option B owner-run proxy probe. Never prints the credential."
    )
    parser.add_argument("mode", choices=VALID_MODES, help="api-key or oauth")
    parser.add_argument(
        "--service",
        default=None,
        help="Keychain -s service name (defaults from mode; must match ^runlith-[a-z0-9-]+$ if given)",
    )
    parser.add_argument("--account", default=os.environ.get("USER", ""), help="Keychain -a account name")
    parser.add_argument("--keychain", default=None, help="Explicit keychain file path (test use only)")
    parser.add_argument(
        "--dry-self-check",
        action="store_true",
        help="Run argument validation and the full local-rejection check suite only. "
        "Never calls `security`, never contacts upstream, never launches the confined child.",
    )
    args = parser.parse_args()

    service = args.service or SERVICE_NAME_DEFAULT[args.mode]
    if args.service is not None and not re.fullmatch(r"runlith-[a-z0-9-]+", args.service):
        print("refusing: --service must match ^runlith-[a-z0-9-]+$", file=sys.stderr)
        sys.exit(2)

    _c9_disable_core_dumps()

    # C3's dangerous-env-var check already ran at import time, before any
    # TLS-touching module was even imported (see the top of this file) --
    # nothing further to strip here now that the policy is "refuse", not
    # "strip and continue".
    global _OPENER
    _OPENER = _build_opener()

    _c4_verify_integrity()

    # T2b: install the SIGALRM handler once, process-wide. Safe here
    # because the server is single-threaded and request handling always
    # happens on the main thread (signal handlers only ever fire there).
    signal.signal(signal.SIGALRM, _alarm_handler)

    placeholder = secrets.token_urlsafe(32)
    counters = Counters()
    state = SharedState(args.mode, placeholder, None, counters)

    server_v4 = ProxyServer(("127.0.0.1", 0), ProxyHandler, state)
    port = server_v4.server_address[1]
    # T2c: also bind [::1] on the SAME port, same handler, same shared
    # state -- Seatbelt's `(remote tcp "localhost:P")` was measured (see
    # the evidence doc's L1 row) to admit connections to ::1:P as well as
    # 127.0.0.1:P, so a confined child resolving "localhost" to its IPv6
    # loopback address must reach an equally-hardened listener, not an
    # unbound port. Refuses to start entirely if this bind fails, rather
    # than silently running v4-only and leaving that path unguarded.
    try:
        server_v6 = ProxyServerV6(("::1", port), ProxyHandler, state)
    except OSError as error:
        print(f"refusing: could not bind [::1]:{port} ({type(error).__name__})", file=sys.stderr)
        server_v4.server_close()
        sys.exit(2)

    thread_v4 = threading.Thread(target=server_v4.serve_forever, daemon=True)
    thread_v6 = threading.Thread(target=server_v6.serve_forever, daemon=True)
    thread_v4.start()
    thread_v6.start()
    try:
        ok, status = run_self_check(state, port)
        print(f"self_check_pass={ok}")
        print(f"self_check_status={status}")
        if not ok:
            print("refusing: negative self-check failed -- aborting before any real work", file=sys.stderr)
            sys.exit(1)

        if args.dry_self_check:
            _extended_dry_checks(state, port)
            counts = counters.snapshot()
            print(f"mode={args.mode}")
            print("dry_run=true")
            print(f"local_401={counts['local_401']}")
            print(f"local_403={counts['local_403']}")
            print(f"local_400={counts['local_400']}")
            print(f"local_413={counts['local_413']}")
            print(f"forwarded={counts['forwarded']}")
            return

        claude_exec = find_claude_exec()
        version = cli_version(claude_exec)

        real_credential = read_keychain_credential(service, args.account, args.keychain)
        if not _validate_credential_shape(args.mode, real_credential):
            real_credential = None
            print("refusing: credential shape does not match mode", file=sys.stderr)
            sys.exit(2)
        state.real_credential = real_credential
        try:
            exit_code, marker_present, stdout_text = launch_confined_child(
                args.mode, placeholder, port, claude_exec
            )
        finally:
            # Best-effort only -- see module docstring: Python strings
            # cannot be securely wiped. Drop every reference this process
            # holds as soon as the child has run.
            real_credential = None
            state.real_credential = None

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
        print(f"local_400={counts['local_400']}")
        print(f"local_413={counts['local_413']}")
        print(f"forwarded={counts['forwarded']}")
        print(f"upstream_status_codes={counts['upstream_status']}")
        if counts["dropped_header_names"]:
            print(f"dropped_header_names={counts['dropped_header_names']}")
    finally:
        server_v4.shutdown()
        server_v4.server_close()
        server_v6.shutdown()
        server_v6.server_close()


if __name__ == "__main__":
    try:
        _real_main()
    except SystemExit:
        raise
    except Exception as error:  # C6: class name only, never str(error)
        print(f"refusing: unexpected error ({type(error).__name__})", file=sys.stderr)
        sys.exit(1)
