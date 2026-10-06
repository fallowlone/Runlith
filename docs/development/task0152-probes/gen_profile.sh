#!/usr/bin/env bash
# TASK-0152 evidence tooling. Generates a macOS Seatbelt profile that
# reproduces, by hand, exactly what
# adapters/runner-macos-seatbelt/src/lib.rs `MacOsSeatbeltRunner::profile_for`
# (base 78079e3, file:line adapters/runner-macos-seatbelt/src/lib.rs:103-248)
# emits for a Claude *provider* launch (RunnerLaunchPurpose::Provider, see
# bins/agentd/src/execution.rs:845 `production_runner_backend(..., RunnerLaunchPurpose::Provider)`).
#
# A provider launch never sets `grant_candidate_ancestor_metadata`
# (execution.rs:154-155), so the emitted profile carries NO
# `file-read-metadata`/`file-test-existence` ancestor block — this script
# omits it exactly as profile_for does for that purpose.
#
# Fixed structural facts reproduced verbatim (profile_for, lib.rs:179-247):
#   (version 1)
#   (deny default)
#   (import "system.sb")
#   (allow process-fork)
#   (allow process-exec (literal <process_paths...>) [(literal <interpreter>)])
#   (allow file-read* file-test-existence file-map-executable (literal <process_paths...>))
#   (allow file-read* file-test-existence (subpath <read_roots...>))      [if any]
#   (allow file-write* (subpath <write_roots...>))                        [if any]
#   (allow network-outbound (literal "/private/var/run/mDNSResponder") (remote tcp "*:443"))
#     -- only when plan.network_destinations() == [NETWORK_SCOPE_OUTBOUND_TCP_443]
#        (runner::NETWORK_SCOPE_OUTBOUND_TCP_443 = "tcp:443", crates/runner/src/lib.rs:26)
#
# read_roots/write_roots derivation (execution.rs:3269-3280, environment.rs:219-237):
#   read_roots  = [workspace] + ProviderEnvironment.isolation_support_read_roots()
#               = [workspace, runtime_root, (HOME/.claude if present), (HOME/.claude.json if present)]
#   write_roots = [workspace] + ProviderEnvironment.isolation_support_write_roots()
#               = [workspace, runtime_root]
# (workspace itself is FileScope::new(workspace, ".") -> workspace_root() root=="."
#  branch, lib.rs:257-259, so the subpath literal is the workspace path itself.)
#
# process_paths = [context.executable] = the CANONICALIZED provider
# executable path. `find_on_path` (adapters/provider-claude/src/detect.rs:108-128)
# resolves `claude` on PATH and returns the PATH entry as-is (the
# `~/.local/bin/claude` symlink itself, unresolved) but
# `ExecutionComposition::resolved_provider_executable`
# (bins/agentd/src/execution.rs:503-521) immediately canonicalizes it with
# `std::fs::canonicalize` and that canonicalized `PathBuf` is the only value
# used downstream (bins/agentd/src/execution.rs:3588 `let executable =
# composition.resolved_provider_executable()?;`, fed to both the security
# spec / process-exec scope and the actual `Command` executable). This
# script therefore canonicalizes --claude-exec the same way before emitting
# any literal. Empirically: a profile granting only the *symlink* path
# denies at the `sandbox-exec` exec boundary itself
# (`execvp() ... failed: Operation not permitted`, before the CLI's own code
# ever runs) -- Seatbelt's `process-exec (literal ...)` match is evaluated
# against the resolved target, not the argv string. Granting the
# canonicalized path alone reaches the CLI's own logic. This is the one
# point where hand reconstruction needed empirical confirmation rather than
# pure static reading, and it is consistent with -- not a deviation from --
# the cited production code path.
#
# This script fails closed (set -euo pipefail) and never invents a rule not
# traceable to the citations above. Every substitution this script makes is
# a caller-supplied concrete path; no structural rule is invented.

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: gen_profile.sh --workspace DIR --claude-exec PATH --runtime-root DIR \
         [--support-read DIR ...] [--network prod443|local:PORT|none] \
         --out FILE

  --workspace DIR      workspace root (read+write subpath)
  --claude-exec PATH   exact provider executable path (process-exec + file-read* literal)
  --runtime-root DIR   provider runtime root (read+write subpath; must exist)
  --support-read DIR    additional read-only subpath (repeatable; e.g. HOME/.claude)
  --network MODE       prod443 (default) | local:PORT | none
  --out FILE           output .sb path
EOF
}

WORKSPACE=""
CLAUDE_EXEC=""
RUNTIME_ROOT=""
declare -a SUPPORT_READ=()
NETWORK="prod443"
OUT=""

while [ $# -gt 0 ]; do
  case "$1" in
    --workspace) WORKSPACE="$2"; shift 2 ;;
    --claude-exec) CLAUDE_EXEC="$2"; shift 2 ;;
    --runtime-root) RUNTIME_ROOT="$2"; shift 2 ;;
    --support-read) SUPPORT_READ+=("$2"); shift 2 ;;
    --network) NETWORK="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

for required in WORKSPACE CLAUDE_EXEC RUNTIME_ROOT OUT; do
  if [ -z "${!required}" ]; then
    echo "missing required --${required,,}" >&2
    exit 2
  fi
done

# checked_absolute-equivalent guard (lib.rs:273-287): reject quotes,
# backslashes, control chars, non-absolute paths.
check_safe_absolute() {
  local p="$1"
  case "$p" in
    /*) : ;;
    *) echo "not absolute: $p" >&2; exit 2 ;;
  esac
  case "$p" in
    *'"'*) echo "contains quote: $p" >&2; exit 2 ;;
    *'\'*) echo "contains backslash: $p" >&2; exit 2 ;;
  esac
}

# Canonicalize the executable exactly as
# ExecutionComposition::resolved_provider_executable does (execution.rs:511
# `std::fs::canonicalize(&executable)`) before it becomes a profile literal.
CLAUDE_EXEC="$(readlink -f "$CLAUDE_EXEC")"

# Canonicalize every other path input too. On macOS, `$TMPDIR`-derived
# paths (`/var/folders/...`) are themselves a symlink to
# `/private/var/folders/...`, and the kernel's own getcwd()/realpath()
# resolution -- which a Seatbelt `subpath` grant must match -- returns the
# resolved physical path, not the symlinked one (measured: `cd` into a
# `/var/folders/...` workspace and `pwd -P` differs from the literal path
# passed in). A profile built from the unresolved path silently denies
# every read/write under it. This mirrors the same class of fix already
# applied to `--claude-exec` above, now applied uniformly to every
# filesystem-scope input so this tool works regardless of which directory
# (assigned scratchpad, a bare `mktemp -d`, etc.) the caller points it at.
WORKSPACE="$(readlink -f "$WORKSPACE")"
RUNTIME_ROOT="$(readlink -f "$RUNTIME_ROOT")"
for i in "${!SUPPORT_READ[@]}"; do
  [ -n "${SUPPORT_READ[$i]}" ] && SUPPORT_READ[$i]="$(readlink -f "${SUPPORT_READ[$i]}")"
done

check_safe_absolute "$WORKSPACE"
check_safe_absolute "$CLAUDE_EXEC"
check_safe_absolute "$RUNTIME_ROOT"
for r in "${SUPPORT_READ[@]:-}"; do
  [ -n "$r" ] && check_safe_absolute "$r"
done

# Ensure the output directory exists before writing -- on a fresh scratch
# tree (e.g. `$SCRATCH/results/`) nothing else in this tool creates it, and
# writing to a missing directory is the exact failure mode this script now
# guards below (see the `||` after the profile-writing block).
mkdir -p "$(dirname "$OUT")"

{
  printf '(version 1)\n(deny default)\n(import "system.sb")\n(allow process-fork)\n'

  printf '(allow process-exec\n  (literal "%s"))\n' "$CLAUDE_EXEC"

  printf '(allow file-read* file-test-existence file-map-executable\n  (literal "%s"))\n' "$CLAUDE_EXEC"

  # read_roots = workspace + runtime_root + support-read extras, sorted+deduped
  # (lib.rs:129-130 read_roots.sort(); read_roots.dedup()).
  read_roots=("$WORKSPACE" "$RUNTIME_ROOT")
  for r in "${SUPPORT_READ[@]:-}"; do
    [ -n "$r" ] && read_roots+=("$r")
  done
  printf '(allow file-read* file-test-existence\n'
  printf '%s\n' "${read_roots[@]}" | sort -u | while IFS= read -r r; do
    printf '  (subpath "%s")\n' "$r"
  done
  printf ')\n'

  # write_roots = workspace + runtime_root only (environment.rs:235-237:
  # isolation_support_write_roots() returns only runtime_root).
  write_roots=("$WORKSPACE" "$RUNTIME_ROOT")
  printf '(allow file-write*\n'
  printf '%s\n' "${write_roots[@]}" | sort -u | while IFS= read -r r; do
    printf '  (subpath "%s")\n' "$r"
  done
  printf ')\n'

  case "$NETWORK" in
    prod443)
      printf '(allow network-outbound\n  (literal "/private/var/run/mDNSResponder")\n  (remote tcp "*:443"))\n'
      ;;
    local:*)
      port="${NETWORK#local:}"
      # macOS Seatbelt's `(remote tcp "HOST:PORT")` accepts only `*` or
      # `localhost` as HOST (measured: sandbox-exec rejects a literal IP
      # with "host must be * or localhost in network address"). "localhost"
      # here does NOT admit only one single loopback interface -- round 6's
      # "L1 loopback admission" evidence row measured that it admits BOTH
      # 127.0.0.1:PORT and [::1]:PORT (and, on this host, other 127/8
      # addresses too), while a non-loopback interface address and a
      # genuinely remote host on the same port are both denied. A listener
      # meant to receive this traffic must itself bind every address this
      # rule admits, not just 127.0.0.1 -- see
      # docs/development/task0152-probes/owner_proxy_probe.py's dual-stack
      # (127.0.0.1 + [::1]) bind for the concrete fix this motivated.
      printf '(allow network-outbound\n  (remote tcp "localhost:%s"))\n' "$port"
      ;;
    none) : ;;
    *)
      echo "unknown --network mode: $NETWORK" >&2
      exit 2
      ;;
  esac
} > "$OUT" || {
  # A failed redirection on a `{ ... } > file` group is a documented bash
  # quirk: it prints its own error but does NOT trip `set -e` (measured
  # during harness hardening -- the script otherwise silently continued
  # past a missing output directory, writing nothing, and the caller then
  # ran `sandbox-exec` against a missing/empty profile file, which failed
  # with an unrelated-looking sandbox-level exit code instead of a clear
  # harness error). The explicit `||` here checks the redirection's exit
  # status directly, which is unaffected by that `set -e` quirk.
  echo "gen_profile.sh: failed to write profile to $OUT" >&2
  exit 1
}
if [ ! -s "$OUT" ]; then
  echo "gen_profile.sh: profile at $OUT is missing or empty after generation" >&2
  exit 1
fi

echo "$OUT"
