#!/usr/bin/env bash
# TASK-0152 R6/R7: local-endpoint mode. Starts stub_server.py on
# 127.0.0.1:<random port>, points the CLI at it via ANTHROPIC_BASE_URL under
# a Seatbelt profile whose ONLY outbound network rule is that localhost
# port (no *:443, no mDNSResponder), and records a sanitized outcome plus
# the stub's sanitized request log.
#
# NOTE ON FIDELITY: the current MacOsSeatbeltRunner::profile_for
# (adapters/runner-macos-seatbelt/src/lib.rs:230-245) can express only
# `[]` (no network) or exactly `NETWORK_SCOPE_OUTBOUND_TCP_443`
# ("tcp:443", crates/runner/src/lib.rs:26) -- any other
# `plan.network_destinations()` value returns
# `RunnerError::BackendUnavailable`. A localhost-only outbound rule is
# therefore NOT a scope the current production Runner backend can compile a
# plan for at all; this script's local-endpoint profile is a manual,
# hand-written extension for this probe only, not a reconstruction of an
# existing production code path. That absence is itself a capability
# finding recorded in the evidence doc.
#
# Usage: run_local_endpoint_row.sh ROW_NAME [--env NAME=VALUE ...] \
#          [--no-credential] [--cred-var NAME]
#
# --cred-var NAME (round 2, R8): which env var carries the dummy
# credential (default ANTHROPIC_API_KEY). Used to probe
# CLAUDE_CODE_OAUTH_TOKEN and ANTHROPIC_AUTH_TOKEN header-shape behavior
# against the local stub the same way R6/R7 probed ANTHROPIC_API_KEY.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRATCH="${TASK0152_SCRATCH:?TASK0152_SCRATCH must be set}"
CLAUDE_EXEC="$(readlink -f "${TASK0152_CLAUDE_EXEC:?TASK0152_CLAUDE_EXEC must be set}")"
MARKER="${TASK0152_MARKER:-TASK0152_EVIDENCE_OK}"
PROMPT="Do not use any tools. Reply with exactly ${MARKER} and nothing else."
DUMMY="runlith-task0152-probe-invalid-credential"

ROW="${1:?row name required}"
shift
declare -a EXTRA_ENV=()
NO_CREDENTIAL=0
CRED_VAR="ANTHROPIC_API_KEY"
while [ $# -gt 0 ]; do
  case "$1" in
    --env) EXTRA_ENV+=("$2"); shift 2 ;;
    --no-credential) NO_CREDENTIAL=1; shift ;;
    --cred-var) CRED_VAR="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

WORKSPACE="$SCRATCH/fixtures/workspace"
RUNTIME_ROOT="$SCRATCH/fixtures/runtime"
mkdir -p "$WORKSPACE" "$RUNTIME_ROOT"/{config,data,state,cache,scratch}
chmod 700 "$RUNTIME_ROOT" 2>/dev/null || true

RAW_DIR="$SCRATCH/raw"
RESULTS_DIR="$SCRATCH/results"
mkdir -p "$RAW_DIR" "$RESULTS_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
STDOUT_RAW="$RAW_DIR/${ROW}-${STAMP}.stdout"
STDERR_RAW="$RAW_DIR/${ROW}-${STAMP}.stderr"
STUB_LOG="$RAW_DIR/${ROW}-${STAMP}.stub.jsonl"
STUB_PORT_FILE="$RAW_DIR/${ROW}-${STAMP}.stub.port"
DENIALS_RAW="$RAW_DIR/${ROW}-${STAMP}.denials"
: > "$STUB_LOG"

export TASK0152_STUB_LOG="$STUB_LOG"
export TASK0152_MARKER="$MARKER"
export TASK0152_DUMMY_CREDENTIAL="$DUMMY"
python3 "$SCRIPT_DIR/stub_server.py" > "$STUB_PORT_FILE" 2>&1 &
STUB_PID=$!
cleanup() { kill "$STUB_PID" 2>/dev/null || true; wait "$STUB_PID" 2>/dev/null || true; }
trap cleanup EXIT

for _ in $(seq 1 50); do
  if grep -q '^PORT=' "$STUB_PORT_FILE" 2>/dev/null; then break; fi
  sleep 0.1
done
PORT="$(grep -o '[0-9]\+' "$STUB_PORT_FILE" | head -1)"
if [ -z "$PORT" ]; then
  echo '{"row":"'"$ROW"'","exit":null,"marker":false,"class":"stub server failed to start","duration_s":0}'
  exit 0
fi

PROFILE="$RESULTS_DIR/${ROW}.sb"
# A profile-generation failure is a HARNESS error, never a probe result --
# see run_row.sh for the matching fix and its rationale (a bash `set -e`
# quirk let a missing output directory pass silently through gen_profile.sh
# and get misreported downstream as an ordinary CLI failure exit code).
if ! GEN_PROFILE_OUT="$(bash "$SCRIPT_DIR/gen_profile.sh" --workspace "$WORKSPACE" --claude-exec "$CLAUDE_EXEC" \
  --runtime-root "$RUNTIME_ROOT" --network "local:$PORT" --out "$PROFILE" 2>&1)"; then
  echo "${GEN_PROFILE_OUT}" >&2
  python3 -c "import json,sys; print(json.dumps({'row': sys.argv[1], 'harness_error': 'profile generation failed', 'exit': None, 'marker': False, 'class': None}))" "$ROW"
  exit 1
fi

declare -a ENV_ARGS=(
  "PATH=/usr/bin:/bin:/usr/sbin:/sbin:$(dirname "$CLAUDE_EXEC")"
  "HOME=$HOME"
  "USER=${USER:-$(id -un)}"
  "PWD=$WORKSPACE"
  "XDG_CONFIG_HOME=$RUNTIME_ROOT/config"
  "XDG_DATA_HOME=$RUNTIME_ROOT/data"
  "XDG_STATE_HOME=$RUNTIME_ROOT/state"
  "XDG_CACHE_HOME=$RUNTIME_ROOT/cache"
  "CLAUDE_CODE_TMPDIR=$RUNTIME_ROOT/scratch"
  "TMPDIR=$RUNTIME_ROOT/scratch"
  "TMP=$RUNTIME_ROOT/scratch"
  "TEMP=$RUNTIME_ROOT/scratch"
  "ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT"
)
if [ "$NO_CREDENTIAL" != "1" ]; then
  ENV_ARGS+=("${CRED_VAR}=${DUMMY}")
fi
for kv in "${EXTRA_ENV[@]:-}"; do
  [ -n "$kv" ] && ENV_ARGS+=("$kv")
done

START_EPOCH=$(date +%s)
set +e
( cd "$WORKSPACE" && env -i "${ENV_ARGS[@]}" \
  /usr/bin/sandbox-exec -p "$(cat "$PROFILE")" \
  "$CLAUDE_EXEC" -p --safe-mode --output-format json "$PROMPT" \
  >"$STDOUT_RAW" 2>"$STDERR_RAW" )
CODE=$?
set -e
END_EPOCH=$(date +%s)

MARKER_PRESENT=false
if grep -q "$MARKER" "$STDOUT_RAW" 2>/dev/null; then
  MARKER_PRESENT=true
fi

# Round 2 (R8): capture denials unconditionally, not only on failure --
# the question here ("did the CLI attempt any OTHER outbound connection,
# e.g. a token-refresh or profile host, even while the main request
# succeeded") specifically needs this on a SUCCESSFUL (marker present) row
# too, not just a failing one.
if command -v log >/dev/null 2>&1; then
  log show --last 2m --style compact \
    --predicate 'eventMessage CONTAINS "deny"' 2>/dev/null \
    | grep -iE "claude|sandbox" > "$DENIALS_RAW" || true
fi
# Sanitized denial summary: operation + target class only (network-outbound
# denials specifically, since that is what "other outbound connection
# attempted" means here) -- never a full raw line, never a path under a
# user secret directory.
NETWORK_DENIAL_COUNT=0
if [ -s "$DENIALS_RAW" ]; then
  NETWORK_DENIAL_COUNT=$(grep -ciE "network-outbound" "$DENIALS_RAW" 2>/dev/null || true)
  [ -z "$NETWORK_DENIAL_COUNT" ] && NETWORK_DENIAL_COUNT=0
fi

cleanup
trap - EXIT

REQUEST_COUNT=$(wc -l < "$STUB_LOG" | tr -d ' ')
# Round 2 (R8) header-shape summary: paths hit, which credential header
# name(s) were used (never values), and the raw (non-secret) value of
# anthropic-beta/anthropic-version the CLI actually sent -- read straight
# from stub_server.py's own sanitized log, not re-derived by guesswork.
STUB_SUMMARY=$(python3 -c "
import json
paths = set()
credential_headers = set()
beta_values = set()
version_values = set()
try:
    with open('$STUB_LOG') as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            entry = json.loads(line)
            paths.add(entry.get('path', ''))
            ch = entry.get('credential_header')
            if ch:
                credential_headers.add(ch)
            safe = entry.get('safe_header_values') or {}
            if 'anthropic-beta' in safe:
                beta_values.add(safe['anthropic-beta'])
            if 'anthropic-version' in safe:
                version_values.add(safe['anthropic-version'])
except FileNotFoundError:
    pass
print(json.dumps({
    'stub_paths_hit': sorted(paths),
    'credential_header_names_seen': sorted(credential_headers),
    'anthropic_beta_values_seen': sorted(beta_values),
    'anthropic_version_values_seen': sorted(version_values),
}))
")

CLASS="unclassified failure (exit ${CODE})"
if [ "$MARKER_PRESENT" = "true" ]; then
  CLASS="success"
elif [ "$REQUEST_COUNT" -eq 0 ]; then
  CLASS="no request reached the stub (denied before any outbound connection, or refused to start without credential)"
elif grep -qiE "not logged in|please run|/login" "$STDOUT_RAW" "$STDERR_RAW" 2>/dev/null; then
  CLASS="not logged in"
elif grep -qiE "invalid.?x-api-key|invalid api key|authentication_error|401" "$STDOUT_RAW" "$STDERR_RAW" 2>/dev/null; then
  CLASS="auth rejected by stub (401/invalid key) -- stub reached, non-conformant response accepted as error"
fi

python3 - "$ROW" "$CRED_VAR" "$CODE" "$MARKER_PRESENT" "$CLASS" "$((END_EPOCH - START_EPOCH))" "$REQUEST_COUNT" "$STUB_SUMMARY" "$NETWORK_DENIAL_COUNT" <<'PYEOF'
import json, sys
row, cred_var, code, marker, cls, dur, count, summary, net_denials = sys.argv[1:10]
result = {
    "row": row,
    "cred_var": cred_var,
    "exit": int(code),
    "marker": marker == "true",
    "class": cls,
    "duration_s": int(dur),
    "stub_request_count": int(count),
    "other_network_denials": int(net_denials),
}
result.update(json.loads(summary))
print(json.dumps(result))
PYEOF
