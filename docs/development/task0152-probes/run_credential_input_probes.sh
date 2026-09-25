#!/usr/bin/env bash
# TASK-0152 (G8) credential-input probe matrix P0-P8.
#
# Runs the installed Claude Code CLI against a local fake Anthropic Messages
# API endpoint (fake_anthropic_endpoint.py) with ONLY dummy, obviously-fake
# marker credentials, never a real one, and never against a real Anthropic
# host. See docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md for the
# question this answers and how to read the output.
#
# Every claude invocation runs with `env -i` and a fresh scratch HOME per
# row: this script never inherits or forwards this host's/container's own
# environment into the child. It never prints, greps, or logs this
# container's own environment variables, and never reads ~/.claude,
# ~/.config/anthropic, a .credentials.json, or a keychain.
#
# The fake endpoint (see its own header comment) records only method, path
# (query stripped), header NAMES, and for x-api-key/authorization/
# anthropic-beta a classification of what they carried - never a raw header
# value. If a probe's log shows "non-dummy-value-present" for x-api-key or
# authorization, this script flags it loudly (STOP/ESCALATE per the task
# envelope) but does not attempt to inspect, hash, or otherwise materialize
# the value; it only ever preserves the classification.
#
# Usage: CLAUDE_BIN=/path/to/claude ./run_credential_input_probes.sh
# Runs on Linux and macOS (bash, POSIX tools, python3 stdlib only).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_BIN="${CLAUDE_BIN:-$(command -v claude 2>/dev/null || true)}"
if [ -z "$CLAUDE_BIN" ]; then
  echo "CLAUDE_BIN not set and no 'claude' found on PATH" >&2
  exit 2
fi

WORKDIR="${TASK0152_PROBE_WORKDIR:-$(mktemp -d "${TMPDIR:-/tmp}/task0152-probes.XXXXXX")}"
mkdir -p "$WORKDIR"
SUMMARY_FILE="$WORKDIR/summary.tsv"
: > "$SUMMARY_FILE"
printf 'row\texit\tmarker\tpaths\tauth_headers\tstderr_class\tleak_flag\n' >> "$SUMMARY_FILE"

PROMPT="Do not use any tools. Reply with exactly TASK0152_PROBE_OK and nothing else."
MARKER="TASK0152_PROBE_OK"

DUMMY_API_KEY="RUNLITH_T0152_DUMMY_API_KEY_NOT_A_SECRET"
DUMMY_BEARER="RUNLITH_T0152_DUMMY_BEARER_NOT_A_SECRET"
DUMMY_OAUTH="RUNLITH_T0152_DUMMY_OAUTH_NOT_A_SECRET"

SAFE_PATH="/opt/node22/bin:/usr/bin:/bin"

log() { printf '%s\n' "$*" >&2; }

resolved_claude_path() {
  python3 - "$CLAUDE_BIN" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
}

find_free_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

wait_for_file() {
  local target="$1" tries=0
  while [ ! -f "$target" ]; do
    tries=$((tries + 1))
    if [ "$tries" -gt 100 ]; then
      log "timed out waiting for $target"
      return 1
    fi
    sleep 0.1
  done
}

stop_pid() {
  local pid="$1"
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" 2>/dev/null || true
}

start_endpoint() {
  # Prints "<pid> <port> <log_file> <ready_file>" on stdout.
  local row="$1"
  local log_file="$WORKDIR/endpoint-$row.jsonl"
  local ready_file="$WORKDIR/ready-$row"
  rm -f "$log_file" "$ready_file" "${log_file}.SECRET_LEAK_DETECTED"
  : > "$log_file"
  local port
  port="$(find_free_port)"
  python3 "$SCRIPT_DIR/fake_anthropic_endpoint.py" \
    --port "$port" --log "$log_file" --ready-file "$ready_file" \
    >"$WORKDIR/endpoint_server-$row.out" 2>&1 &
  local pid=$!
  wait_for_file "$ready_file"
  printf '%s %s %s %s\n' "$pid" "$port" "$log_file" "$ready_file"
}

paths_from_log() {
  local log_file="$1"
  [ -f "$log_file" ] || { echo "-"; return; }
  python3 - "$log_file" <<'PY'
import json, sys
paths = []
with open(sys.argv[1]) as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except ValueError:
            continue
        p = f"{rec.get('method')} {rec.get('path')}"
        if p not in paths:
            paths.append(p)
print(",".join(paths) if paths else "-")
PY
}

auth_from_log() {
  local log_file="$1"
  [ -f "$log_file" ] || { echo "-"; return; }
  python3 - "$log_file" <<'PY'
import json, sys
seen = {}
with open(sys.argv[1]) as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except ValueError:
            continue
        auth = rec.get("auth", {})
        for h in ("x-api-key", "authorization"):
            v = auth.get(h)
            if v and v != "absent":
                seen[h] = v
print(";".join(f"{k}={v}" for k, v in seen.items()) if seen else "-")
PY
}

classify_stderr() {
  local err_file="$1"
  if [ ! -s "$err_file" ]; then
    echo "empty"
    return
  fi
  local first_line
  first_line="$(head -c 400 "$err_file" | tr '\n' ' ' | head -c 120)"
  echo "snippet: $first_line"
}

check_leak() {
  local log_file="$1" row="$2"
  if [ -f "${log_file}.SECRET_LEAK_DETECTED" ]; then
    log "!!!! STOP/ESCALATE: row $row recorded a non-dummy value in an auth-relevant header (x-api-key or authorization). See ${log_file}.SECRET_LEAK_DETECTED - it holds only the classification, never a raw value. This means something other than this script's own dummy markers reached the CLI's outbound request. !!!!"
    echo "LEAK_FLAG"
  else
    echo "-"
  fi
}

record_result() {
  local row="$1" exit_code="$2" out_file="$3" err_file="$4" log_file="$5"
  local marker="no"
  grep -q "$MARKER" "$out_file" 2>/dev/null && marker="yes" || true
  local paths auth stderr_class leak
  paths="$(paths_from_log "$log_file")"
  auth="$(auth_from_log "$log_file")"
  stderr_class="$(classify_stderr "$err_file")"
  leak="$(check_leak "$log_file" "$row")"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$row" "$exit_code" "$marker" "$paths" "$auth" "$stderr_class" "$leak" >> "$SUMMARY_FILE"
  log "== $row == exit=$exit_code marker=$marker paths=$paths auth=$auth stderr=[$stderr_class] leak=$leak"
}

# run_probe ROW SAFE_MODE(yes|no) EXTRA_ARGV_FILE_OR_EMPTY ENV_ASSIGNMENT...
# EXTRA_ARGV_FILE_OR_EMPTY: path to a file with one extra claude argv token
# per line (e.g. "--settings" / the settings JSON), or "" for none.
run_probe() {
  local row="$1" safe_mode="$2" extra_argv_file="$3"
  shift 3
  local home_dir="$WORKDIR/home-$row"
  local work_dir="$WORKDIR/work-$row"
  local out_file="$WORKDIR/stdout-$row.txt"
  local err_file="$WORKDIR/stderr-$row.txt"
  rm -rf "$home_dir" "$work_dir"
  mkdir -p "$home_dir" "$work_dir"

  local endpoint_info pid port log_file ready_file
  endpoint_info="$(start_endpoint "$row")"
  pid="$(echo "$endpoint_info" | cut -d' ' -f1)"
  port="$(echo "$endpoint_info" | cut -d' ' -f2)"
  log_file="$(echo "$endpoint_info" | cut -d' ' -f3)"
  ready_file="$(echo "$endpoint_info" | cut -d' ' -f4)"

  local -a base_argv=(-p)
  if [ "$safe_mode" = "yes" ]; then
    base_argv+=(--safe-mode)
  fi
  base_argv+=(--output-format json)
  local -a extra_argv=()
  if [ -n "$extra_argv_file" ] && [ -f "$extra_argv_file" ]; then
    while IFS= read -r line; do extra_argv+=("$line"); done < "$extra_argv_file"
  fi

  set +e
  (
    cd "$work_dir" && \
    timeout 45 env -i PATH="$SAFE_PATH" HOME="$home_dir" \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$port" \
      "$@" \
      "$CLAUDE_BIN" "${base_argv[@]}" "${extra_argv[@]}" "$PROMPT"
  ) >"$out_file" 2>"$err_file"
  local exit_code=$?
  set -e

  stop_pid "$pid"
  record_result "$row" "$exit_code" "$out_file" "$err_file" "$log_file"
}

# run_fd_probe ROW VAR_NAME DUMMY_VALUE
# Delivers the dummy value through a file descriptor named VAR_NAME (a
# *_FILE_DESCRIPTOR-style variable): opens fd 3 on a scratch file holding the
# dummy value and sets VAR_NAME=3 in the child's environment.
run_fd_probe() {
  local row="$1" var_name="$2" dummy_value="$3"
  local home_dir="$WORKDIR/home-$row"
  local work_dir="$WORKDIR/work-$row"
  local out_file="$WORKDIR/stdout-$row.txt"
  local err_file="$WORKDIR/stderr-$row.txt"
  local fd_source="$WORKDIR/fdsrc-$row.txt"
  rm -rf "$home_dir" "$work_dir"
  mkdir -p "$home_dir" "$work_dir"
  printf '%s' "$dummy_value" > "$fd_source"

  local endpoint_info pid port log_file ready_file
  endpoint_info="$(start_endpoint "$row")"
  pid="$(echo "$endpoint_info" | cut -d' ' -f1)"
  port="$(echo "$endpoint_info" | cut -d' ' -f2)"
  log_file="$(echo "$endpoint_info" | cut -d' ' -f3)"
  ready_file="$(echo "$endpoint_info" | cut -d' ' -f4)"

  set +e
  (
    cd "$work_dir" && \
    exec 3<"$fd_source" && \
    eval "timeout 45 env -i PATH=\"$SAFE_PATH\" HOME=\"$home_dir\" \
      ANTHROPIC_BASE_URL=\"http://127.0.0.1:$port\" \
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
      $var_name=3 \
      \"$CLAUDE_BIN\" -p --safe-mode --output-format json \"\$PROMPT\""
  ) >"$out_file" 2>"$err_file"
  local exit_code=$?
  set -e

  stop_pid "$pid"
  record_result "$row" "$exit_code" "$out_file" "$err_file" "$log_file"
}

# run_credfile_probe ROW VAR_NAME DUMMY_VALUE
# Delivers the dummy value through a *_FILE-style variable pointing at a
# scratch file (mode 600) holding the dummy value.
run_credfile_probe() {
  local row="$1" var_name="$2" dummy_value="$3"
  local home_dir="$WORKDIR/home-$row"
  local work_dir="$WORKDIR/work-$row"
  local out_file="$WORKDIR/stdout-$row.txt"
  local err_file="$WORKDIR/stderr-$row.txt"
  local cred_file="$WORKDIR/credfile-$row.txt"
  rm -rf "$home_dir" "$work_dir"
  mkdir -p "$home_dir" "$work_dir"
  printf '%s' "$dummy_value" > "$cred_file"
  chmod 600 "$cred_file"

  local endpoint_info pid port log_file ready_file
  endpoint_info="$(start_endpoint "$row")"
  pid="$(echo "$endpoint_info" | cut -d' ' -f1)"
  port="$(echo "$endpoint_info" | cut -d' ' -f2)"
  log_file="$(echo "$endpoint_info" | cut -d' ' -f3)"
  ready_file="$(echo "$endpoint_info" | cut -d' ' -f4)"

  set +e
  (
    cd "$work_dir" && \
    timeout 45 env -i PATH="$SAFE_PATH" HOME="$home_dir" \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$port" \
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
      "$var_name=$cred_file" \
      "$CLAUDE_BIN" -p --safe-mode --output-format json "$PROMPT"
  ) >"$out_file" 2>"$err_file"
  local exit_code=$?
  set -e

  stop_pid "$pid"
  record_result "$row" "$exit_code" "$out_file" "$err_file" "$log_file"
}

p5_settings_hook() {
  # assets_dir must NOT be under a probe's home_dir/work_dir: run_probe wipes
  # those with `rm -rf` before every launch, which would delete this helper
  # script and the generated --settings argv file out from under it.
  local assets_dir="$1"
  mkdir -p "$assets_dir"
  cat > "$assets_dir/apikeyhelper.sh" <<EOF
#!/bin/sh
echo "$DUMMY_API_KEY"
EOF
  chmod 700 "$assets_dir/apikeyhelper.sh"
  python3 - "$assets_dir" > "$assets_dir/extra_argv.list" <<'PY'
import json, sys
assets_dir = sys.argv[1]
settings = json.dumps({"apiKeyHelper": f"{assets_dir}/apikeyhelper.sh"})
print("--settings")
print(settings)
PY
}

log "=== TASK-0152 credential-input probe matrix ==="
log "CLAUDE_BIN=$CLAUDE_BIN"
RESOLVED_CLAUDE="$(resolved_claude_path)"
log "resolved executable=$RESOLVED_CLAUDE"
mkdir -p "$WORKDIR/home-version"
CLAUDE_VERSION="$(env -i PATH="$SAFE_PATH" HOME="$WORKDIR/home-version" "$CLAUDE_BIN" --version 2>&1 || true)"
log "claude --version => $CLAUDE_VERSION"
log "work dir: $WORKDIR"
log ""

# P0: no credential at all (+ base URL only).
run_probe P0_no_credential yes "" CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1

# P1: ANTHROPIC_API_KEY dummy.
run_probe P1_api_key yes "" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  "ANTHROPIC_API_KEY=$DUMMY_API_KEY"

# P2: ANTHROPIC_AUTH_TOKEN dummy.
run_probe P2_auth_token yes "" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  "ANTHROPIC_AUTH_TOKEN=$DUMMY_BEARER"

# P3: CLAUDE_CODE_OAUTH_TOKEN dummy.
run_probe P3_oauth_token yes "" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  "CLAUDE_CODE_OAUTH_TOKEN=$DUMMY_OAUTH"

# P4: precedence, ANTHROPIC_API_KEY + ANTHROPIC_AUTH_TOKEN together.
run_probe P4_api_plus_auth yes "" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  "ANTHROPIC_API_KEY=$DUMMY_API_KEY" \
  "ANTHROPIC_AUTH_TOKEN=$DUMMY_BEARER"

# P4: precedence, ANTHROPIC_API_KEY + CLAUDE_CODE_OAUTH_TOKEN together.
run_probe P4_api_plus_oauth yes "" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  "ANTHROPIC_API_KEY=$DUMMY_API_KEY" \
  "CLAUDE_CODE_OAUTH_TOKEN=$DUMMY_OAUTH"

# P5: apiKeyHelper via --settings, with and without --safe-mode. The helper
# script and generated --settings argv file live in a dedicated assets dir,
# never inside a probe's home_dir/work_dir (run_probe wipes those first).
assets_p5a="$WORKDIR/assets-P5_apikeyhelper_safe"
p5_settings_hook "$assets_p5a"
run_probe P5_apikeyhelper_safe yes "$assets_p5a/extra_argv.list" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1

assets_p5b="$WORKDIR/assets-P5_apikeyhelper_unsafe"
p5_settings_hook "$assets_p5b"
run_probe P5_apikeyhelper_unsafe no "$assets_p5b/extra_argv.list" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1

# P6: file-descriptor / file-based credential variables. Only variable names
# actually found via a grep of the installed CLI binary are probed here; see
# the doc for the exact "found vs probed vs honoured" table.
P6_FD_NAMES="CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR"
for var_name in $P6_FD_NAMES; do
  run_fd_probe "P6_fd_${var_name}" "$var_name" "$DUMMY_API_KEY"
done

P6_FILE_NAMES="ANTHROPIC_IDENTITY_TOKEN_FILE CLAUDE_CODE_HOST_CREDS_FILE"
for var_name in $P6_FILE_NAMES; do
  run_credfile_probe "P6_file_${var_name}" "$var_name" "$DUMMY_API_KEY"
done

# P7: egress inventory. Route through a logging CONNECT proxy that never
# tunnels; run once without CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC and
# once with it, both with ANTHROPIC_API_KEY dummy and ANTHROPIC_BASE_URL
# pointed at the loopback fake endpoint.
run_p7() {
  local row="$1" disable_nonessential="$2"
  local home_dir="$WORKDIR/home-$row"
  local work_dir="$WORKDIR/work-$row"
  local out_file="$WORKDIR/stdout-$row.txt"
  local err_file="$WORKDIR/stderr-$row.txt"
  local proxy_log="$WORKDIR/proxylog-$row.jsonl"
  local proxy_ready="$WORKDIR/proxyready-$row"
  rm -rf "$home_dir" "$work_dir"
  mkdir -p "$home_dir" "$work_dir"
  rm -f "$proxy_log" "$proxy_ready"
  : > "$proxy_log"

  local proxy_port
  proxy_port="$(find_free_port)"
  python3 "$SCRIPT_DIR/logging_connect_proxy.py" \
    --port "$proxy_port" --log "$proxy_log" --ready-file "$proxy_ready" \
    >"$WORKDIR/proxy_server-$row.out" 2>&1 &
  local proxy_pid=$!
  wait_for_file "$proxy_ready"

  local endpoint_info pid port log_file ready_file
  endpoint_info="$(start_endpoint "$row")"
  pid="$(echo "$endpoint_info" | cut -d' ' -f1)"
  port="$(echo "$endpoint_info" | cut -d' ' -f2)"
  log_file="$(echo "$endpoint_info" | cut -d' ' -f3)"
  ready_file="$(echo "$endpoint_info" | cut -d' ' -f4)"

  local -a extra_env=("ANTHROPIC_API_KEY=$DUMMY_API_KEY" \
    "HTTPS_PROXY=http://127.0.0.1:$proxy_port" \
    "HTTP_PROXY=http://127.0.0.1:$proxy_port" \
    "NO_PROXY=127.0.0.1,localhost")
  if [ "$disable_nonessential" = "yes" ]; then
    extra_env+=("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1")
  fi

  set +e
  (
    cd "$work_dir" && \
    timeout 45 env -i PATH="$SAFE_PATH" HOME="$home_dir" \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$port" \
      "${extra_env[@]}" \
      "$CLAUDE_BIN" -p --safe-mode --output-format json "$PROMPT"
  ) >"$out_file" 2>"$err_file"
  local exit_code=$?
  set -e

  stop_pid "$pid"
  stop_pid "$proxy_pid"
  record_result "$row" "$exit_code" "$out_file" "$err_file" "$log_file"

  local hosts
  hosts="$(python3 - "$proxy_log" <<'PY'
import json, sys
hosts = []
try:
    with open(sys.argv[1]) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except ValueError:
                continue
            target = rec.get("target", "")
            host = target.split(":")[0]
            if host and host not in ("127.0.0.1", "localhost") and host not in hosts:
                hosts.append(host)
except FileNotFoundError:
    pass
print(",".join(hosts) if hosts else "-")
PY
)"
  log "== $row egress hosts (non-loopback CONNECT targets, refused, no tunnel) == $hosts"
  printf '%s\tegress_hosts=%s\n' "$row" "$hosts" >> "$WORKDIR/egress_hosts.tsv"
}

: > "$WORKDIR/egress_hosts.tsv"
run_p7 P7_egress_with_nonessential no
run_p7 P7_egress_no_nonessential yes

log ""
log "=== summary ($SUMMARY_FILE) ==="
if command -v column >/dev/null 2>&1; then
  column -t -s"$(printf '\t')" "$SUMMARY_FILE" >&2
else
  cat "$SUMMARY_FILE" >&2
fi

log ""
log "=== egress hosts (P7, $WORKDIR/egress_hosts.tsv) ==="
cat "$WORKDIR/egress_hosts.tsv" >&2

any_leak="no"
if grep -q "LEAK_FLAG" "$SUMMARY_FILE"; then
  any_leak="yes"
fi
log ""
log "any_row_flagged_non_dummy_value=$any_leak"
log "work dir preserved at: $WORKDIR"
