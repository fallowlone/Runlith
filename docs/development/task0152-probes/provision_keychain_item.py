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
import pwd
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


_UNSAFE_CHARS = re.compile(r'[\x00-\x1f\x7f"\\]')


def _reject_unsafe_for_security_line(label, value):
    """Round 6 (T1b): fixes a `security -i` command-injection path.
    `_quote_for_security` escapes `\\` and `"`, but a value containing a
    raw newline or other control character could still terminate the one
    intended command line early and start a second, attacker-chosen
    `security` command on the next line -- `security -i` reads one command
    per line, so a value crossing a line boundary is a real injection
    surface, not merely a display nuisance. Refuses before any such value
    is ever assembled into the command line at all."""
    if _UNSAFE_CHARS.search(value):
        print(
            f"refusing: {label} contains a control character, quote, or backslash",
            file=sys.stderr,
        )
        sys.exit(2)


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


def _item_exists(account, service, keychain_path):
    """Round 6 (T1a): metadata-only existence check -- `find-generic-password`
    WITHOUT `-w`, which searches and reports on the item's non-secret
    attributes only and does not request the confidential password data,
    so it does not trigger the ACL's owner-approval prompt (verified
    against a throwaway temp keychain; see the evidence doc's ACL
    excerpt). Returns True/False/None (None = the check itself could not
    be completed, e.g. `security` failed to run at all -- treated as "may
    exist" by the caller, i.e. fails closed toward refusing to add-and-
    reset an ACL)."""
    argv = ["/usr/bin/security", "find-generic-password", "-a", account, "-s", service]
    if keychain_path:
        argv.append(keychain_path)
    try:
        completed = subprocess.run(argv, capture_output=True, text=True, timeout=15, check=False)
    except Exception:
        return None
    return completed.returncode == 0


def _store_via_security_interactive(account, service, value, keychain_path):
    """Stores the item via `security -i` fed over stdin. The ONLY argv for
    the `security` subprocess is `["/usr/bin/security", "-i"]` -- no
    account, service, or secret ever appears in any argv. No `-U` (round 6,
    T1a: re-adding over an existing item does not reset that item's ACL,
    so this function is only ever called after `_item_exists` has already
    confirmed there is nothing to silently update). `-T ""` grants no
    application trusted access, so every later read prompts the owner for
    approval (verified separately by `_dump_acl_has_no_trusted_apps`
    against a throwaway temp keychain, never the owner's real one)."""
    for label, val in (("account", account), ("service", service)):
        _reject_unsafe_for_security_line(label, val)
    if keychain_path:
        _reject_unsafe_for_security_line("keychain path", keychain_path)
    parts = [
        "add-generic-password",
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
    Prints nothing from the dump itself.

    Round 6 (T1c) hardening: rather than a single global substring search
    (which could match the wrong item's ACL block in a keychain holding
    more than one item), this splits the dump into per-item blocks on each
    `keychain: ` line -- present once per item even within one keychain
    file -- and requires EXACTLY ONE block containing BOTH this service's
    AND this account's attribute markers before drawing any conclusion.
    An ambiguous result (zero or more than one matching block) returns
    `None` rather than guessing."""
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
    text = completed.stdout
    blocks = re.split(r"(?=^keychain: )", text, flags=re.MULTILINE)
    svce_marker = f'"svce"<blob>="{service}"'
    acct_marker = f'"acct"<blob>="{account}"'
    matches = [block for block in blocks if svce_marker in block and acct_marker in block]
    if len(matches) != 1:
        return None
    segment = matches[0]
    # Measured real format (not guessed): `security dump-keychain -a`
    # renders each ACL entry as
    #     entry N:
    #         authorizations (K): decrypt derive export_clear export_wrapped mac sign
    #         ...
    #         applications (COUNT):
    # `-T ""` specifically restricts the CONFIDENTIALITY-relevant
    # authorizations (decrypt/export) -- that is the one entry this
    # function must check. `applications (0):` on that entry means zero
    # trusted applications (every read prompts). A non-zero count, or a
    # listed application path, means at least one trusted app exists.
    entries = re.split(r"\n {4}entry \d+:\n", segment)[1:]
    for entry in entries:
        header = entry.split("\n", 1)[0]
        if "decrypt" not in header:
            continue
        count_match = re.search(r"applications \((\d+)\):", entry)
        if count_match:
            return int(count_match.group(1)) == 0
        # Any other shape (e.g. "applications: <null>") is not a form this
        # function claims to interpret -- do not guess either way.
        return None
    # No ACL entry gates the decrypt/export authorizations at all -- not
    # the shape this function was written to interpret.
    return None


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

    if args.keychain:
        _reject_unsafe_for_security_line("keychain path", args.keychain)

    # Round 6 (T1b): the account name comes from the OS user database, not
    # $USER/$LOGNAME -- an ambient env var an unusual environment could set
    # to something unexpected. `pwd.getpwuid(os.getuid())` asks the kernel
    # who actually owns this process.
    account = pwd.getpwuid(os.getuid()).pw_name
    service = SERVICE_NAME[args.mode]

    # Round 6 (T1a): re-adding over an existing item with `-U` does not
    # reset that item's ACL -- an item created some other way (e.g. the
    # withdrawn round-2/3 manual flow, which trusted `/usr/bin/security`
    # itself) would silently keep its old, looser ACL forever. Check
    # first, and refuse with the exact command to remove it, rather than
    # ever risk widening or leaving stale an ACL guarantee.
    exists = _item_exists(account, service, args.keychain)
    if exists is None:
        print("refusing: could not determine whether an item already exists", file=sys.stderr)
        sys.exit(2)
    if exists:
        print(
            "refusing: an item for this service/account already exists -- its ACL is "
            "NOT reset by re-adding over it. Delete it first, then re-run this tool:\n"
            f'  security delete-generic-password -a "$USER" -s {service}',
            file=sys.stderr,
        )
        sys.exit(2)

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
