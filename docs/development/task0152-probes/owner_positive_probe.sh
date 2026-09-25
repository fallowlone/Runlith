#!/usr/bin/env bash
# TASK-0152 owner-run positive probe. NOT run by the implementer agent --
# the owner runs this manually, with their own real credential, after
# creating a Keychain item themselves (see the one-time setup command in
# docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md, which puts `-w` as
# the LAST argument with no value so `security` prompts interactively and
# the value never appears in argv or shell history).
#
# What it does:
#   1. Reads the named Keychain generic-password item on the HOST, outside
#      any sandbox (`security find-generic-password -w`; this call's own
#      argv holds no secret -- `-w` only requests the value on stdout).
#   2. Injects that value into a confined child, via ONLY the env var name
#      the R2-R5 probes proved the installed CLI honours, under the exact
#      production-shaped Seatbelt profile (gen_profile.sh) -- but with a
#      FRESH, empty HOME and CLAUDE_CONFIG_DIR under this run's own mktemp
#      directory (same pattern as probe row R5), and no support-read grant
#      on any host `~/.claude`/`~/.claude.json` at all. A pass therefore
#      proves the mechanism needs no pre-existing host session state, not
#      merely that it works with one.
#   3. Runs the fixed harmless marker prompt, non-interactively.
#   4. Prints ONLY: CLI version, exit status, marker present/absent, and a
#      classified failure reason. Never the credential value, never raw
#      provider stdout/stderr, never account/session identifiers.
#
# Safety:
#   - `set +x` always; this script never enables xtrace.
#   - The credential value is never placed in the argv of ANY process.
#     `env NAME=value cmd` (the naive approach) puts the pair in `env`'s own
#     argv, which is `ps`-visible for the (small but real) window before
#     `env` execs into `cmd`. This script instead runs the launch inside a
#     subshell that clears every inherited exported variable
#     (`compgen -e` + `unset`), re-`export`s only the allowlisted names
#     (a shell builtin -- builtins never appear as a separate `ps` argv),
#     and then `exec`s `sandbox-exec` directly -- no `env` process, and no
#     argv anywhere ever contains the credential.
#   - The credential value lives in one local variable for the shortest
#     possible scope, is never `echo`ed, `printf`ed, logged, or written to
#     any file, and is `unset` before the script exits.
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
# Fresh, empty HOME and CLAUDE_CONFIG_DIR under this run's own mktemp dir --
# same pattern probe row R5 used. No host `~/.claude`/`~/.claude.json` is
# granted or referenced anywhere in this script: a pass here proves the
# mechanism needs no pre-existing host session state.
FRESH_HOME="$WORKDIR/home"
CLAUDE_CONFIG_DIR_VALUE="$FRESH_HOME/.claude-config"
mkdir -p "$WORKSPACE" "$RUNTIME_ROOT"/{config,data,state,cache,scratch} "$FRESH_HOME"
chmod 700 "$RUNTIME_ROOT" "$FRESH_HOME"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

PROFILE="$WORKDIR/production-shaped.sb"
bash "$SCRIPT_DIR/gen_profile.sh" --workspace "$WORKSPACE" --claude-exec "$CLAUDE_EXEC" \
  --runtime-root "$RUNTIME_ROOT" --network prod443 --out "$PROFILE" >/dev/null

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

# Values needed inside the subshell below, captured now (before that
# subshell strips its own exported environment) so they survive as plain
# subshell-inherited shell variables even though they are not exported.
LAUNCH_PATH="/usr/bin:/bin:/usr/sbin:/sbin:$(dirname "$CLAUDE_EXEC")"
LAUNCH_XDG_CONFIG="$RUNTIME_ROOT/config"
LAUNCH_XDG_DATA="$RUNTIME_ROOT/data"
LAUNCH_XDG_STATE="$RUNTIME_ROOT/state"
LAUNCH_XDG_CACHE="$RUNTIME_ROOT/cache"
LAUNCH_TMP="$RUNTIME_ROOT/scratch"

set +e
(
  cd "$WORKSPACE" || exit 125
  # Clear every exported variable this subshell inherited (ambient
  # credentials, cloud config, PATH, HOME, everything) -- the `env -i`
  # equivalent, but done with the `unset` builtin so no external process
  # ever sees these names/values as argv. `compgen -e` itself is a builtin
  # too; nothing here spawns a process before the final `exec`.
  while IFS= read -r exported_name; do
    unset "$exported_name" 2>/dev/null || true
  done < <(compgen -e)

  # Re-establish only the explicit allowlisted pairs, plus the credential
  # variable under test, via the `export` builtin -- never via `env`'s
  # argv, so the credential value is never written into any process's
  # command line.
  export PATH="$LAUNCH_PATH"
  export HOME="$FRESH_HOME"
  export USER="$ACCOUNT"
  export PWD="$WORKSPACE"
  export XDG_CONFIG_HOME="$LAUNCH_XDG_CONFIG"
  export XDG_DATA_HOME="$LAUNCH_XDG_DATA"
  export XDG_STATE_HOME="$LAUNCH_XDG_STATE"
  export XDG_CACHE_HOME="$LAUNCH_XDG_CACHE"
  export CLAUDE_CODE_TMPDIR="$LAUNCH_TMP"
  export TMPDIR="$LAUNCH_TMP"
  export TMP="$LAUNCH_TMP"
  export TEMP="$LAUNCH_TMP"
  export CLAUDE_CONFIG_DIR="$CLAUDE_CONFIG_DIR_VALUE"
  export "${VAR_NAME}=${CREDENTIAL_VALUE}"

  exec /usr/bin/sandbox-exec -p "$(cat "$PROFILE")" \
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
