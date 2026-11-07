#!/usr/bin/env bash
# TASK-0152 (G8) - macOS Seatbelt credential-input probe matrix, rows M1-M6.
#
# NOT executed by the implementer: this repository worktree has no macOS
# host available. This script is prepared for the owner to run on macOS and
# is not part of this task's executed evidence. `bash -n` on this file is
# part of this task's verification; the M1-M6 runtime rows are not.
#
# Profile generation mirrors MacOsSeatbeltRunner::profile_for at commit
# 78079e36a5f463093ea07418f23f19f2f2ef3d87,
# adapters/runner-macos-seatbelt/src/lib.rs lines 103-249, in particular:
#   - lines 103-131 workspace/process-path/read-write root assembly;
#   - lines 190-213 the deny-default base, process-exec and
#     file-read*/file-map-executable rules for the launch executable(s);
#   - lines 214-223 the read-root and write-root subpath rules;
#   - lines 224-244 (`match plan.network_destinations()`) the network rule:
#     the single supported non-empty scope
#     (`NETWORK_SCOPE_OUTBOUND_TCP_443`, crates/runner/src/lib.rs) emits
#     exactly:
#       (allow network-outbound
#         (literal "/private/var/run/mDNSResponder")
#         (remote tcp "*:443"))
#     any other requested scope is refused by the real backend
#     (`BackendUnavailable`); this script's "loopback:<port>" mode is a
#     LOCAL PROBE CONVENIENCE ONLY, not a scope the production backend can
#     emit - it exists so the owner can prove the CLI still authenticates
#     when only a single loopback port is reachable, without opening the
#     production `*:443` scope for the probe itself.
#
# This script intentionally NEVER emits the three additional Seatbelt rules
# G7_TASK_0112 found necessary for host-Keychain-based provider auth:
#   (allow mach-lookup (global-name "com.apple.SecurityServer"))
#   (allow file-read-data (literal "$HOME/Library/Keychains/login.keychain-db"))
#   (allow file-read* file-test-existence file-map-executable (literal "/usr/bin/security"))
#   (allow process-exec (literal "/usr/bin/security"))
# The whole point of this matrix is to show what the CLI does WITHOUT those
# grants, using only dummy, obviously-fake marker credentials delivered
# through the non-Keychain input channels TASK-0152's Linux run identified.
#
# Usage:
#   ./macos_seatbelt_matrix.sh --claude-bin /path/to/claude [--claude-bin /path/to/resolved-claude] \
#     [--read-root /extra/read/root ...] [--write-root /extra/write/root ...] \
#     --network tcp443|loopback:<port> [--workspace /path] [--dry-run] [--run-m6]
#
# `--dry-run` prints the generated profile for the given options and exits
# without invoking sandbox-exec or the CLI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

CLAUDE_BINS=()
READ_ROOTS=()
WRITE_ROOTS=()
NETWORK_MODE=""
WORKSPACE=""
DRY_RUN="no"
RUN_M6="no"

usage() {
  cat <<'EOF'
Usage: macos_seatbelt_matrix.sh --claude-bin PATH [--claude-bin PATH ...]
         --network (tcp443|loopback:PORT) [--read-root PATH ...]
         [--write-root PATH ...] [--workspace DIR] [--dry-run] [--run-m6]

--claude-bin may be repeated; pass both the launcher path and its resolved
  real executable, matching this repository's process_paths handling.
--network tcp443 mirrors the production network-outbound rule exactly
  (`(remote tcp "*:443")` + mDNSResponder). --network loopback:PORT is a
  probe-only convenience that instead allows only
  `(remote tcp "localhost:PORT")`.
--read-root / --write-root add extra Seatbelt read/write subpaths beyond the
  workspace (mirrors ProviderEnvironment::isolation_support_read_roots /
  isolation_support_write_roots, adapters/provider-claude/src/environment.rs
  lines 219-237).
--dry-run prints the generated profile and exits.
--run-m6 additionally runs the manual, interactive M6 row (owner /login
  step). Off by default.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --claude-bin) CLAUDE_BINS+=("$2"); shift 2 ;;
    --read-root) READ_ROOTS+=("$2"); shift 2 ;;
    --write-root) WRITE_ROOTS+=("$2"); shift 2 ;;
    --network) NETWORK_MODE="$2"; shift 2 ;;
    --workspace) WORKSPACE="$2"; shift 2 ;;
    --dry-run) DRY_RUN="yes"; shift ;;
    --run-m6) RUN_M6="yes"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ "${#CLAUDE_BINS[@]}" -eq 0 ]; then
  echo "at least one --claude-bin is required" >&2
  exit 2
fi
if [ -z "$NETWORK_MODE" ]; then
  echo "--network is required (tcp443 or loopback:PORT)" >&2
  exit 2
fi
if [ -z "$WORKSPACE" ]; then
  WORKSPACE="$(mktemp -d "${TMPDIR:-/tmp}/task0152-seatbelt.XXXXXX")"
fi
mkdir -p "$WORKSPACE"

sb_literal() {
  # Emits a Seatbelt (literal "...") clause for one path, escaping the two
  # characters Seatbelt string literals care about.
  local path="$1"
  local escaped="${path//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf '  (literal "%s")\n' "$escaped"
}

sb_subpath() {
  local path="$1"
  local escaped="${path//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf '  (subpath "%s")\n' "$escaped"
}

generate_profile() {
  # Mirrors adapters/runner-macos-seatbelt/src/lib.rs profile_for (lines
  # 103-249) at 78079e36a5f463093ea07418f23f19f2f2ef3d87. This is a
  # hand-mirrored reimplementation for the probe harness, not the crate
  # itself - the owner is trusted to diff it against lib.rs if the two
  # drift.
  local profile
  profile="(version 1)
(deny default)
(import \"system.sb\")
(allow process-fork)
"
  profile+="(allow process-exec
"
  for bin in "${CLAUDE_BINS[@]}"; do
    profile+="$(sb_literal "$bin")"$'\n'
  done
  profile+=")
"
  profile+="(allow file-read* file-test-existence file-map-executable
"
  for bin in "${CLAUDE_BINS[@]}"; do
    profile+="$(sb_literal "$bin")"$'\n'
  done
  profile+=")
"

  profile+="(allow file-read* file-test-existence
"
  profile+="$(sb_subpath "$WORKSPACE")"$'\n'
  for root in "${READ_ROOTS[@]}"; do
    profile+="$(sb_subpath "$root")"$'\n'
  done
  profile+=")
"

  profile+="(allow file-write*
"
  profile+="$(sb_subpath "$WORKSPACE")"$'\n'
  for root in "${WRITE_ROOTS[@]}"; do
    profile+="$(sb_subpath "$root")"$'\n'
  done
  profile+=")
"

  case "$NETWORK_MODE" in
    tcp443)
      profile+='(allow network-outbound
  (literal "/private/var/run/mDNSResponder")
  (remote tcp "*:443"))
'
      ;;
    loopback:*)
      local port="${NETWORK_MODE#loopback:}"
      profile+="(allow network-outbound
  (remote tcp \"localhost:$port\"))
"
      ;;
    *)
      echo "unsupported --network value: $NETWORK_MODE (use tcp443 or loopback:PORT)" >&2
      exit 2
      ;;
  esac

  printf '%s' "$profile"
}

PROFILE="$(generate_profile)"

if [ "$DRY_RUN" = "yes" ]; then
  printf '%s\n' "$PROFILE"
  exit 0
fi

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this script's non-dry-run rows require macOS (sandbox-exec); refusing to run them here" >&2
  exit 2
fi

DUMMY_API_KEY="RUNLITH_T0152_DUMMY_API_KEY_NOT_A_SECRET"
DUMMY_BEARER="RUNLITH_T0152_DUMMY_BEARER_NOT_A_SECRET"
PROMPT="Do not use any tools. Reply with exactly TASK0152_PROBE_OK and nothing else."
MARKER="TASK0152_PROBE_OK"
CLAUDE_BIN="${CLAUDE_BINS[0]}"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/task0152-seatbelt-run.XXXXXX")"
PROFILE_FILE="$WORKDIR/profile.sb"
printf '%s\n' "$PROFILE" > "$PROFILE_FILE"

log() { printf '%s\n' "$*" >&2; }

run_row() {
  local row="$1" network_mode="$2"; shift 2
  local home_dir="$WORKDIR/home-$row"
  local out_file="$WORKDIR/stdout-$row.txt"
  local err_file="$WORKDIR/stderr-$row.txt"
  local log_file="$WORKDIR/endpoint-$row.jsonl"
  local ready_file="$WORKDIR/ready-$row"
  rm -rf "$home_dir"
  mkdir -p "$home_dir"
  : > "$log_file"
  rm -f "$ready_file" "${log_file}.SECRET_LEAK_DETECTED"

  local endpoint_port endpoint_pid
  case "$network_mode" in
    tcp443)
      endpoint_port=443
      ;;
    loopback:*)
      endpoint_port="${network_mode#loopback:}"
      ;;
  esac

  python3 "$SCRIPT_DIR/fake_anthropic_endpoint.py" \
    --port "$endpoint_port" --log "$log_file" --ready-file "$ready_file" \
    >"$WORKDIR/endpoint_server-$row.out" 2>&1 &
  endpoint_pid=$!
  local tries=0
  while [ ! -f "$ready_file" ]; do
    tries=$((tries + 1))
    if [ "$tries" -gt 100 ]; then
      log "endpoint did not become ready for row $row (binding 127.0.0.1:$endpoint_port may need elevated privilege on this macOS version)"
      kill "$endpoint_pid" >/dev/null 2>&1 || true
      return 1
    fi
    sleep 0.1
  done

  set +e
  timeout 45 /usr/bin/sandbox-exec -f "$PROFILE_FILE" \
    env -i PATH=/usr/bin:/bin HOME="$home_dir" \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$endpoint_port" \
      "$@" \
      "$CLAUDE_BIN" -p --safe-mode --output-format json "$PROMPT" \
    >"$out_file" 2>"$err_file"
  local exit_code=$?
  set -e

  kill "$endpoint_pid" >/dev/null 2>&1 || true
  wait "$endpoint_pid" 2>/dev/null || true

  local marker="no"
  grep -q "$MARKER" "$out_file" 2>/dev/null && marker="yes" || true
  local leak="-"
  if [ -f "${log_file}.SECRET_LEAK_DETECTED" ]; then
    leak="LEAK_FLAG"
    log "!!!! STOP/ESCALATE: row $row recorded a non-dummy value in an auth-relevant header. See ${log_file}.SECRET_LEAK_DETECTED (classification only). !!!!"
  fi
  log "== $row == exit=$exit_code marker=$marker leak=$leak (endpoint log: $log_file)"
}

log "profile written to: $PROFILE_FILE"
log "work dir: $WORKDIR"

log "-- M1: API key dummy under tcp443 --"
run_row M1_api_key_tcp443 tcp443 "ANTHROPIC_API_KEY=$DUMMY_API_KEY"

log "-- M2: AUTH_TOKEN dummy under tcp443 --"
run_row M2_auth_token_tcp443 tcp443 "ANTHROPIC_AUTH_TOKEN=$DUMMY_BEARER"

case "$NETWORK_MODE" in
  loopback:*)
    log "-- M3: API key dummy under loopback-only (+ CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1) --"
    run_row M3_api_key_loopback "$NETWORK_MODE" \
      "ANTHROPIC_API_KEY=$DUMMY_API_KEY" "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"

    log "-- M4: AUTH_TOKEN dummy under loopback-only --"
    run_row M4_auth_token_loopback "$NETWORK_MODE" \
      "ANTHROPIC_AUTH_TOKEN=$DUMMY_BEARER" "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"
    ;;
  *)
    log "-- M3/M4 (loopback-only rows) skipped: run this script again with --network loopback:<port> to cover them --"
    ;;
esac

log "-- M5: control, no credential, confirm refusal without Keychain --"
run_row M5_no_credential_control "$NETWORK_MODE"

if [ "$RUN_M6" = "yes" ]; then
  log "-- M6: manual owner step - interactive /login inside the confined profile, Keychain denied --"
  log "This row is interactive and is NOT run automatically by this script beyond"
  log "the steps below; it requires a human at the terminal."
  m6_config_dir="$WORKDIR/m6-claude-config"
  mkdir -p "$m6_config_dir"
  log "1. In a separate confined shell, run:"
  log "   sandbox-exec -f \"$PROFILE_FILE\" env -i PATH=/usr/bin:/bin HOME=\"$WORKDIR/home-M6\" CLAUDE_CONFIG_DIR=\"$m6_config_dir\" \"$CLAUDE_BIN\" /login"
  log "2. Complete the interactive login. Keychain access must be denied by this"
  log "   profile (no SecurityServer mach-lookup, no login.keychain-db read, no"
  log "   /usr/bin/security exec) - a login that silently falls back to some other"
  log "   store is itself a finding to report."
  log "3. Re-run this script's M6 check below once login is complete."
  cred_file="$m6_config_dir/.credentials.json"
  if [ -e "$cred_file" ]; then
    if stat -f '%Lp' "$cred_file" >/dev/null 2>&1; then
      mode="$(stat -f '%Lp' "$cred_file")"
    else
      mode="$(stat -c '%a' "$cred_file" 2>/dev/null || echo unknown)"
    fi
    log "credentials file exists: yes, mode=$mode (content never read by this script)"
    log "-- M6b: confined marker call against the REAL endpoint, network tcp443, read/write scoped to $m6_config_dir only --"
    m6b_profile_file="$WORKDIR/profile-m6b.sb"
    m6b_read_roots=("${READ_ROOTS[@]}" "$m6_config_dir")
    m6b_write_roots=("${WRITE_ROOTS[@]}" "$m6_config_dir")
    READ_ROOTS=("${m6b_read_roots[@]}")
    WRITE_ROOTS=("${m6b_write_roots[@]}")
    NETWORK_MODE="tcp443"
    generate_profile > "$m6b_profile_file"
    set +e
    timeout 45 /usr/bin/sandbox-exec -f "$m6b_profile_file" \
      env -i PATH=/usr/bin:/bin HOME="$WORKDIR/home-M6" CLAUDE_CONFIG_DIR="$m6_config_dir" \
      "$CLAUDE_BIN" -p --safe-mode --output-format json "$PROMPT" \
      >"$WORKDIR/stdout-M6b.txt" 2>"$WORKDIR/stderr-M6b.txt"
    m6b_exit=$?
    set -e
    m6b_marker="no"
    grep -q "$MARKER" "$WORKDIR/stdout-M6b.txt" 2>/dev/null && m6b_marker="yes" || true
    log "== M6b == exit=$m6b_exit marker=$m6b_marker"
  else
    log "credentials file exists: no"
  fi
fi

log "done"
