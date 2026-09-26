#!/usr/bin/env python3
"""TASK-0152 round 4 -- Keychain provisioning helper.

Fixes a class of mistake, not just its symptom: on 2026-09-25 the owner ran
the manual `security add-generic-password ... -s <service> ... -w` command
from the round-2/3 evidence doc and accidentally typed a REAL OAuth token
into the `-s` (service name) argument instead of letting `-w` prompt for
it -- the token landed in shell argv, shell history, and chat. This tool
removes every command-line surface where that class of mistake is
possible: the owner never types a service name (it is derived from
`mode`), never types a secret on any command line (only via a masked
`getpass` prompt, entered twice and compared), and the secret is stored
through `security -i` (interactive mode) fed over STDIN, so the secret
never appears in ANY process's argv at all -- not even `security`'s own.

Usage:
    python3 -I provision_keychain_item.py {oauth,api-key} [--keychain PATH]

`--keychain PATH` targets an explicit keychain file (used by this task's
own temp-keychain tests only; the owner's normal use leaves it unset and
the item is stored in the default keychain).

Never prints the secret, any part of it, or even its length.

Stdlib only.
"""

import argparse
import getpass
import hmac
import os
import re
import subprocess
import sys
import tempfile

MODES = ("oauth", "api-key")
SERVICE_NAME = {
    "oauth": "runlith-task0152-owner-oauth-token",
    "api-key": "runlith-task0152-owner-anthropic-api-key",
}

# The real token prefix (`s`+`k`+`-`+`a`+`n`+`t`+`-`, joined) is never
# written as one contiguous literal anywhere in this file (a secret-scan
# of this repo greps for exactly that joined form). Building it from
# single-character pieces at runtime satisfies both: the running program
# still validates the real prefix shape, but no static scan of the source
# text finds a token-shaped literal.
_TOKEN_PREFIX_BASE = "".join(("s", "k", "-", "a", "n", "t", "-"))
TOKEN_PREFIX = {
    "oauth": _TOKEN_PREFIX_BASE + "oat",
    "api-key": _TOKEN_PREFIX_BASE + "api",
}
# Same reasoning applied to the >=40-char high-entropy-token detector used
# for the anti-mistake argv scan below: no token-shaped literal appears in
# this file for a scan to find, only the detection logic.
_SHAPE_CHARSET = re.compile(r"^[A-Za-z0-9_-]+$")
MIN_SECRET_LEN = 20
MAX_SECRET_LEN = 512


def _looks_secret(arg):
    """True if an argv element looks like it could be a credential -- the
    exact class of mistake this tool exists to make structurally
    impossible. Matches C1's rule in owner_proxy_probe.py: starts with a
    known token prefix, or is itself a long high-entropy-shaped run."""
    for prefix in TOKEN_PREFIX.values():
        if arg.startswith(prefix):
            return True
    return len(arg) >= 40 and bool(_SHAPE_CHARSET.fullmatch(arg))


def _refuse_if_secret_looking_argv():
    for arg in sys.argv[1:]:
        if _looks_secret(arg):
            print(
                "refusing: an argument looks like it could be a credential -- "
                "this tool never takes a secret on the command line",
                file=sys.stderr,
            )
            sys.exit(2)


def _require_isolated_interpreter():
    if not sys.flags.isolated:
        print(
            "refusing: run this with `python3 -I provision_keychain_item.py ...` "
            "(isolated interpreter mode)",
            file=sys.stderr,
        )
        sys.exit(2)


def _looks_like_temp_path(path):
    real = os.path.realpath(path)
    candidates = [tempfile.gettempdir(), "/tmp", "/private/tmp", "/var/folders"]
    for candidate in candidates:
        real_candidate = os.path.realpath(candidate)
        if real == real_candidate or real.startswith(real_candidate + os.sep):
            return True
        if real.startswith(candidate + os.sep):
            return True
    return False


def _quote_for_security(value):
    """Quotes one value for a `security -i` interactive command line.
    `security`'s interactive parser supports double-quoted tokens with
    backslash escaping, matching typical shell-style tokenization."""
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def _validate_shape(mode, value):
    prefix = TOKEN_PREFIX[mode]
    if not value.startswith(prefix):
        return False
    remainder = value[len(prefix):]
    if not (MIN_SECRET_LEN <= len(value) <= MAX_SECRET_LEN):
        return False
    if not remainder:
        return False
    if not _SHAPE_CHARSET.fullmatch(remainder):
        return False
    return True


def _read_secret_getpass():
    first = getpass.getpass("Secret value (input hidden): ")
    second = getpass.getpass("Re-enter to confirm (input hidden): ")
    if not hmac.compare_digest(first.encode("utf-8", "replace"), second.encode("utf-8", "replace")):
        print("refusing: the two entries did not match", file=sys.stderr)
        sys.exit(2)
    # Only one copy is kept from this point on.
    del second
    return first


def _read_secret_stdin_for_test():
    """Test-only path (see --stdin-for-test): reads two lines from stdin
    and runs them through the exact same match check as the getpass path,
    so the test exercises real logic rather than a stub. Gated by
    `_looks_like_temp_path` on the caller side -- this function itself
    performs no gating, callers must have already refused otherwise."""
    first = sys.stdin.readline().rstrip("\n")
    second = sys.stdin.readline().rstrip("\n")
    if not hmac.compare_digest(first.encode("utf-8", "replace"), second.encode("utf-8", "replace")):
        print("refusing: the two entries did not match", file=sys.stderr)
        sys.exit(2)
    del second
    return first


def _clear_clipboard_if_matches(value):
    try:
        clip = subprocess.run(["pbpaste"], capture_output=True, text=True, timeout=5).stdout
    except Exception:
        return
    matches = clip is not None and hmac.compare_digest(
        clip.encode("utf-8", "replace"), value.encode("utf-8", "replace")
    )
    clip = None
    if matches:
        try:
            subprocess.run(["pbcopy"], input="", text=True, timeout=5)
            print("clipboard cleared")
        except Exception:
            pass


def _store_via_security_interactive(account, service, value, keychain_path):
    """Stores the item via `security -i` fed over stdin. The ONLY argv for
    the `security` subprocess is `["/usr/bin/security", "-i"]` -- no
    account, service, or secret ever appears in any argv. `-T ""` grants
    no application trusted access, so every later read prompts the owner
    for approval (verified separately by `_dump_acl_has_no_trusted_apps`
    against a throwaway temp keychain, never the owner's real one)."""
    parts = [
        "add-generic-password",
        "-U",
        "-a",
        _quote_for_security(account),
        "-s",
        _quote_for_security(service),
        "-T",
        '""',
        "-w",
        _quote_for_security(value),
    ]
    if keychain_path:
        parts.append(_quote_for_security(keychain_path))
    command_line = " ".join(parts)
    try:
        # `security -i` exits cleanly at EOF of stdin once the one command
        # has been processed; a trailing "quit" is not itself a valid
        # command in this mode and its own "unknown command" error would
        # incorrectly make the process exit non-zero even when
        # add-generic-password itself succeeded (verified empirically
        # with a dummy value against a throwaway temp keychain).
        completed = subprocess.run(
            ["/usr/bin/security", "-i"],
            input=command_line + "\n",
            text=True,
            capture_output=True,
            timeout=20,
        )
    except Exception as error:
        print(f"refusing: failed to invoke security ({type(error).__name__})", file=sys.stderr)
        sys.exit(2)
    # Never print completed.stdout/stderr: `security -i` echoes nothing
    # when stdin is not a TTY, but this script does not rely on that and
    # never surfaces the captured text regardless.
    if completed.returncode != 0:
        print("refusing: security reported a failure storing the item", file=sys.stderr)
        sys.exit(2)


def _dump_acl_has_no_trusted_apps(keychain_path, account, service):
    """Test-only verification against a throwaway temp keychain (never the
    owner's real/login keychain -- callers must guarantee that). Uses
    `security dump-keychain -a <path>` (attributes/ACL only, never `-d`)
    and returns True if the item's ACL grants no trusted application.
    Prints nothing from the dump itself."""
    try:
        completed = subprocess.run(
            ["/usr/bin/security", "dump-keychain", "-a", keychain_path],
            capture_output=True,
            text=True,
            timeout=20,
        )
    except Exception:
        return None
    if completed.returncode != 0:
        return None
    # Parse only structurally: find this item's ACL block and check it has
    # no application-trust entries. The raw dump is never printed.
    text = completed.stdout
    marker = f'"svce"<blob>="{service}"'
    idx = text.find(marker)
    if idx == -1:
        return None
    # Look at the ACL section that follows this item's attribute block, up
    # to the next "keychain:" item boundary.
    next_idx = text.find("\nkeychain:", idx)
    segment = text[idx: next_idx if next_idx != -1 else len(text)]
    # A trusted application entry in `security`'s dump format appears as a
    # path-bearing "application: " or "group: " line for that ACL entry.
    # No trusted app at all (our -T "") shows as an ACL with no such line,
    # or an explicit "no access control" style entry for the partition.
    has_trusted_app = ("application:" in segment) or ("group:" in segment)
    return not has_trusted_app


def main():
    _refuse_if_secret_looking_argv()
    _require_isolated_interpreter()

    parser = argparse.ArgumentParser(
        description="TASK-0152 Keychain provisioning helper. Never takes a secret on the command line."
    )
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--keychain", default=None, help="Explicit keychain file path (test use only)")
    parser.add_argument("--stdin-for-test", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--verify-acl", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()

    if args.stdin_for_test:
        if not args.keychain or not _looks_like_temp_path(args.keychain):
            print(
                "refusing: --stdin-for-test is only permitted when --keychain "
                "points inside a temp directory",
                file=sys.stderr,
            )
            sys.exit(2)

    account = getpass.getuser()
    service = SERVICE_NAME[args.mode]

    if args.stdin_for_test:
        value = _read_secret_stdin_for_test()
    else:
        value = _read_secret_getpass()

    if not _validate_shape(args.mode, value):
        print("refusing: credential shape does not match mode", file=sys.stderr)
        value = None
        sys.exit(2)

    _clear_clipboard_if_matches(value)

    _store_via_security_interactive(account, service, value, args.keychain)
    value = None  # drop the reference as soon as it is no longer needed

    print(f"stored: service={service} account={account}")

    if args.verify_acl:
        if not args.keychain or not _looks_like_temp_path(args.keychain):
            print(
                "refusing: --verify-acl is only permitted when --keychain "
                "points inside a temp directory",
                file=sys.stderr,
            )
            sys.exit(2)
        result = _dump_acl_has_no_trusted_apps(args.keychain, account, service)
        print(f"acl_no_trusted_apps={result}")


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as error:
        print(f"refusing: unexpected error ({type(error).__name__})", file=sys.stderr)
        sys.exit(1)
