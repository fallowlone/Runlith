#!/usr/bin/env bash
# TASK-0152 owner-run positive probe. NOT run by the implementer agent --
# the owner runs this manually, with their own real credential, after
# creating a Keychain item themselves (see the one-time setup command in
# docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md, which uses an
# interactive `read -s` prompt so the credential value never appears in
# shell history).
#
# What it does:
#   1. Reads the named Keychain generic-password item on the HOST, outside
#      any sandbox (`security find-generic-password -w`).
#   2. Injects that value into a confined child, via ONLY the env var name
#      the R2-R5 probes proved the installed CLI honours, under the exact
#      production-shaped Seatbelt profile (gen_profile.sh).
#   3. Runs the fixed harmless marker prompt, non-interactively.
#   4. Prints ONLY: CLI version, exit status, marker present/absent, and a
#      classified failure reason. Never the credential value, never raw
#      provider stdout/stderr, never account/session identifiers.
#
# Safety:
#   - `set +x` always; this script never enables xtrace.
#   - The credential value lives in one local variable for the shortest
#     possible scope, is never `echo`ed, `printf`ed, logged, or written to
#     any file, and is `unset` (and the underlying shell buffer overwritten
#     where the shell supports it) before the script exits.
#   - Refuses to run unless the requested variable name is one of the
#     names this task's probes actually proved honoured (not merely a
#     strings/help-text candidate).
#   - Refuses to run over a value that looks like a real secret shape this
#     script itself will handle wrong (empty) -- it does NOT pattern-match
#     the value's shape beyond emptiness, because doing so would require
#     printing/logging the value to explain a rejection, which this script
#     must never do.

set +x
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat >&2 <<'EOF'
Usage: owner_positive_probe.sh VAR_NAME KEYCHAIN_SERVICE [KEYCHAIN_ACCOUNT]

  VAR_NAME           One of: ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN,
                      ANTHROPIC_AUTH_TOKEN (the names TASK-0152 R2-R5 proved
                      the installed CLI honours without Keychain/SecurityServer
                      authority -- see the evidence doc).
  KEYCHAIN_SERVICE    The `-s` service name of a generic-password item you
                      created yourself (see the evidence doc's one-time setup
                      command).
  KEYCHAIN_ACCOUNT    Optional `-a` account name. Defaults to $USER.

This script never prints the credential value. It prints only: CLI version,
exit status, marker present/absent, and a classified failure reason.
EOF
}

# Allowlist is intentionally hardcoded, not read from any file this script
# doesn't control, so a tampered evidence doc cannot widen it.
PROVEN_HONOURED_VARS=("ANTHROPIC_API_KEY" "CLAUDE_CODE_OAUTH_TOKEN" "ANTHROPIC_AUTH_TOKEN")

VAR_NAME="${1:-}"
SERVICE="${2:-}"
ACCOUNT="${3:-${USER:-$(id -un)}}"

if [ -z "$VAR_NAME" ] || [ -z "$SERVICE" ]; then
  usage
  exit 2
fi

allowed=0
for v in "${PROVEN_HONOURED_VARS[@]}"; do
  if [ "$v" = "$VAR_NAME" ]; then
    allowed=1
    break
  fi
done
if [ "$allowed" -ne 1 ]; then
  echo "refusing: '$VAR_NAME' is not a TASK-0152 proven-honoured variable name" >&2
  echo "proven-honoured: ${PROVEN_HONOURED_VARS[*]}" >&2
  exit 2
fi

CLAUDE_EXEC="$(command -v claude 2>/dev/null || true)"
if [ -z "$CLAUDE_EXEC" ]; then
  echo "refusing: claude executable not found on PATH" >&2
  exit 2
fi
CLAUDE_EXEC="$(readlink -f "$CLAUDE_EXEC")"
CLI_VERSION="$("$CLAUDE_EXEC" --version 2>/dev/null || echo unknown)"

WORKDIR="$(mktemp -d)"
WORKSPACE="$WORKDIR/workspace"
RUNTIME_ROOT="$WORKDIR/runtime"
mkdir -p "$WORKSPACE" "$RUNTIME_ROOT"/{config,data,state,cache,scratch}
chmod 700 "$RUNTIME_ROOT"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

PROFILE="$WORKDIR/production-shaped.sb"
declare -a GEN_ARGS=(--workspace "$WORKSPACE" --claude-exec "$CLAUDE_EXEC" --runtime-root "$RUNTIME_ROOT" --network prod443 --out "$PROFILE")
[ -e "$HOME/.claude" ] && GEN_ARGS+=(--support-read "$HOME/.claude")
[ -e "$HOME/.claude.json" ] && GEN_ARGS+=(--support-read "$HOME/.claude.json")
bash "$SCRIPT_DIR/gen_profile.sh" "${GEN_ARGS[@]}" >/dev/null

MARKER="TASK0152_OWNER_PROBE_OK"
PROMPT="Do not use any tools. Reply with exactly ${MARKER} and nothing else."

# Read the credential on the HOST, outside any sandbox. This is the one
# place this script touches Keychain, and it never leaves this process:
# not printed, not logged, not written to a file, only exported into the
# child's environment below.
CREDENTIAL_VALUE="$(/usr/bin/security find-generic-password -a "$ACCOUNT" -s "$SERVICE" -w 2>/dev/null || true)"
if [ -z "$CREDENTIAL_VALUE" ]; then
  echo "refusing: no Keychain item found for service='$SERVICE' account='$ACCOUNT' (or it is empty)" >&2
  exit 2
fi

STDOUT_FILE="$WORKDIR/stdout"
STDERR_FILE="$WORKDIR/stderr"

set +e
(
  cd "$WORKSPACE" && env -i \
    "PATH=/usr/bin:/bin:/usr/sbin:/sbin:$(dirname "$CLAUDE_EXEC")" \
    "HOME=$HOME" \
    "USER=$ACCOUNT" \
    "PWD=$WORKSPACE" \
    "XDG_CONFIG_HOME=$RUNTIME_ROOT/config" \
    "XDG_DATA_HOME=$RUNTIME_ROOT/data" \
    "XDG_STATE_HOME=$RUNTIME_ROOT/state" \
    "XDG_CACHE_HOME=$RUNTIME_ROOT/cache" \
    "CLAUDE_CODE_TMPDIR=$RUNTIME_ROOT/scratch" \
    "TMPDIR=$RUNTIME_ROOT/scratch" \
    "TMP=$RUNTIME_ROOT/scratch" \
    "TEMP=$RUNTIME_ROOT/scratch" \
    "${VAR_NAME}=${CREDENTIAL_VALUE}" \
    /usr/bin/sandbox-exec -p "$(cat "$PROFILE")" \
    "$CLAUDE_EXEC" -p --safe-mode --output-format json "$PROMPT" \
    >"$STDOUT_FILE" 2>"$STDERR_FILE"
)
CODE=$?
set -e

# Drop the credential from this process's memory as soon as the child has
# been launched and waited on.
CREDENTIAL_VALUE=""
unset CREDENTIAL_VALUE

MARKER_PRESENT=false
grep -q "$MARKER" "$STDOUT_FILE" 2>/dev/null && MARKER_PRESENT=true

CLASS="unclassified"
if [ "$MARKER_PRESENT" = "true" ]; then
  CLASS="success"
elif grep -qiE "not logged in|please run|/login" "$STDOUT_FILE" "$STDERR_FILE" 2>/dev/null; then
  CLASS="not logged in"
elif grep -qiE "invalid.?x-api-key|invalid api key|authentication_error|401|invalid bearer token|incorrect api key" "$STDOUT_FILE" "$STDERR_FILE" 2>/dev/null; then
  CLASS="auth rejected by API (401/invalid key)"
elif grep -qiE "operation not permitted" "$STDERR_FILE" 2>/dev/null; then
  CLASS="sandbox denial"
elif grep -qiE "ECONNREFUSED|ENOTFOUND|ETIMEDOUT|network|fetch failed" "$STDOUT_FILE" "$STDERR_FILE" 2>/dev/null; then
  CLASS="network unreachable"
fi

echo "cli_version=${CLI_VERSION}"
echo "exit_status=${CODE}"
echo "marker_present=${MARKER_PRESENT}"
echo "failure_class=${CLASS}"
