#!/usr/bin/env bash
# TASK-0152: runs one probe row against the installed Claude Code CLI under
# a hand-reconstructed production-shaped macOS Seatbelt profile (see
# gen_profile.sh for provenance) and records ONLY a sanitized outcome:
# exit status, marker present/absent, and a classified failure reason.
#
# It never prints, logs, or persists raw provider stdout/stderr, credential
# values, account identifiers, or request/response bodies. Raw captures are
# written under $RESULTS_DIR/raw/ (scratch only, gitignored by location,
# never read back for anything other than local classification within this
# same process) and are NOT part of the evidence doc.
#
# Usage:
#   run_row.sh ROW_NAME [--network prod443|local:PORT|none] \
#              [--home DIR] [--runtime-root DIR] [--no-support-read] \
#              [--env NAME=VALUE ...] [--extra-argv ARG ...]
#
# Prints one line of JSON to stdout:
#   {"row":"R1","exit":1,"marker":false,"class":"...","note":"..."}

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRATCH="${TASK0152_SCRATCH:?TASK0152_SCRATCH must be set to the scratch results dir}"
# Canonicalized exactly as ExecutionComposition::resolved_provider_executable
# does (execution.rs:511) and as gen_profile.sh does for the profile
# literal: the actual exec target must match the profile's process-exec
# literal, which is the resolved path (see gen_profile.sh provenance note).
CLAUDE_EXEC="$(readlink -f "${TASK0152_CLAUDE_EXEC:?TASK0152_CLAUDE_EXEC must be set}")"
MARKER="${TASK0152_MARKER:-TASK0152_EVIDENCE_OK}"
PROMPT="Do not use any tools. Reply with exactly ${MARKER} and nothing else."

ROW="${1:?row name required}"
shift

NETWORK="prod443"
HOME_DIR="$HOME"
RUNTIME_ROOT="$SCRATCH/fixtures/runtime"
SUPPORT_READ=1
declare -a EXTRA_ENV=()
declare -a EXTRA_ARGV=()

while [ $# -gt 0 ]; do
  case "$1" in
    --network) NETWORK="$2"; shift 2 ;;
    --home) HOME_DIR="$2"; shift 2 ;;
    --runtime-root) RUNTIME_ROOT="$2"; shift 2 ;;
    --no-support-read) SUPPORT_READ=0; shift ;;
    --env) EXTRA_ENV+=("$2"); shift 2 ;;
    --extra-argv) EXTRA_ARGV+=("$2"); shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

WORKSPACE="$SCRATCH/fixtures/workspace"
mkdir -p "$WORKSPACE" "$RUNTIME_ROOT"
chmod 700 "$RUNTIME_ROOT" 2>/dev/null || true
mkdir -p "$RUNTIME_ROOT"/{config,data,state,cache,scratch}

RAW_DIR="$SCRATCH/raw"
mkdir -p "$RAW_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
STDOUT_RAW="$RAW_DIR/${ROW}-${STAMP}.stdout"
STDERR_RAW="$RAW_DIR/${ROW}-${STAMP}.stderr"
DENIALS_RAW="$RAW_DIR/${ROW}-${STAMP}.denials"

PROFILE="$SCRATCH/results/${ROW}.sb"
declare -a GEN_ARGS=(--workspace "$WORKSPACE" --claude-exec "$CLAUDE_EXEC" --runtime-root "$RUNTIME_ROOT" --network "$NETWORK" --out "$PROFILE")
if [ "$SUPPORT_READ" = "1" ]; then
  [ -e "$HOME_DIR/.claude" ] && GEN_ARGS+=(--support-read "$HOME_DIR/.claude")
  [ -e "$HOME_DIR/.claude.json" ] && GEN_ARGS+=(--support-read "$HOME_DIR/.claude.json")
fi
if [ "$NETWORK" = "none" ]; then
  : # host-only / no-sandbox rows call this script with SKIP_SANDBOX=1 instead
fi
bash "$SCRIPT_DIR/gen_profile.sh" "${GEN_ARGS[@]}" >/dev/null

declare -a ENV_ARGS=(
  "PATH=/usr/bin:/bin:/usr/sbin:/sbin:$(dirname "$CLAUDE_EXEC")"
  "HOME=$HOME_DIR"
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
)
for kv in "${EXTRA_ENV[@]:-}"; do
  [ -n "$kv" ] && ENV_ARGS+=("$kv")
done

START_EPOCH=$(date +%s)
set +e
# The real spawn path (adapters/provider-claude/src/process.rs
# `wire.current_dir(workdir)`) sets the *actual OS process cwd* to the
# workspace, not just a PWD env var. getcwd()/realpath() resolution walks
# the real cwd regardless of PWD, so the child must actually be started
# with cwd == workspace or it spuriously tries to read the invoking
# shell's cwd (observed once during harness development: a Seatbelt denial
# on the agent's own worktree path, unrelated to any auth mechanism).
if [ "${#EXTRA_ARGV[@]}" -gt 0 ]; then
  ( cd "$WORKSPACE" && env -i "${ENV_ARGS[@]}" \
    /usr/bin/sandbox-exec -p "$(cat "$PROFILE")" \
    "$CLAUDE_EXEC" -p --safe-mode --output-format json "${EXTRA_ARGV[@]}" "$PROMPT" \
    >"$STDOUT_RAW" 2>"$STDERR_RAW" )
else
  ( cd "$WORKSPACE" && env -i "${ENV_ARGS[@]}" \
    /usr/bin/sandbox-exec -p "$(cat "$PROFILE")" \
    "$CLAUDE_EXEC" -p --safe-mode --output-format json "$PROMPT" \
    >"$STDOUT_RAW" 2>"$STDERR_RAW" )
fi
CODE=$?
set -e
END_EPOCH=$(date +%s)

MARKER_PRESENT=false
if grep -q "$MARKER" "$STDOUT_RAW" 2>/dev/null; then
  MARKER_PRESENT=true
fi

# Optional Seatbelt denial capture for a failing row (operation + target
# class only -- never a path under a user secret directory).
if [ "$MARKER_PRESENT" = "false" ] && command -v log >/dev/null 2>&1; then
  log show --last 2m --style compact \
    --predicate 'eventMessage CONTAINS "deny"' 2>/dev/null \
    | grep -iE "claude|sandbox" > "$DENIALS_RAW" || true
fi

classify() {
  local stdout_file="$1" stderr_file="$2" code="$3"
  if [ "$MARKER_PRESENT" = "true" ]; then
    echo "success"
    return
  fi
  if [ "$code" = "71" ] && grep -q "execvp" "$stderr_file" 2>/dev/null; then
    echo "sandbox denial (process-exec)"
    return
  fi
  if grep -qiE "operation not permitted" "$stderr_file" 2>/dev/null; then
    echo "sandbox denial (filesystem or other operation)"
    return
  fi
  if grep -qiE "not logged in|please run|/login|no credentials" "$stdout_file" "$stderr_file" 2>/dev/null; then
    echo "not logged in"
    return
  fi
  if grep -qiE "invalid.?x-api-key|invalid api key|authentication_error|401|invalid bearer token|incorrect api key" "$stdout_file" "$stderr_file" 2>/dev/null; then
    echo "auth rejected by API (401/invalid key)"
    return
  fi
  if grep -qiE "ECONNREFUSED|ENOTFOUND|ETIMEDOUT|network|fetch failed|could not connect" "$stdout_file" "$stderr_file" 2>/dev/null; then
    echo "network unreachable"
    return
  fi
  if [ "$((END_EPOCH - START_EPOCH))" -ge 55 ]; then
    echo "timeout"
    return
  fi
  if [ "$code" = "1" ]; then
    echo "unclassified CLI failure (exit 1, matches TASK-0112 pattern)"
    return
  fi
  echo "unclassified failure (exit ${code})"
}

CLASS="$(classify "$STDOUT_RAW" "$STDERR_RAW" "$CODE")"

# Sanitized, retained record only.
python3 - "$ROW" "$CODE" "$MARKER_PRESENT" "$CLASS" "$((END_EPOCH - START_EPOCH))" <<'PYEOF'
import json, sys
row, code, marker, cls, dur = sys.argv[1:6]
print(json.dumps({
    "row": row,
    "exit": int(code),
    "marker": marker == "true",
    "class": cls,
    "duration_s": int(dur),
}))
PYEOF
