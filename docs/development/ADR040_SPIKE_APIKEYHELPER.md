# Spike: `apiKeyHelper` as a vault-credential channel (ADR-0040)

Date: 2026-10-05. CLI measured: 2.1.290. Method: measurement only; no
production path changed, no repo files touched during probing.

## Setup

- Isolated env: `env -i` with `PATH`, empty-`HOME`, `CLAUDE_CODE_SIMPLE=1`
  (forces `ANTHROPIC_API_KEY`-or-`apiKeyHelper` auth; host OAuth/keychain
  never read), plus a `RUNLITH_SPIKE_LOG` pointer for the helper.
- `/tmp/runlith-spike-apikeyhelper/settings.json`:
  `{"apiKeyHelper": "/tmp/.../helper.sh"}`.
- Helper: appends `argc`/argv and env var NAMES only (never values) to a
  log file, prints the dummy key `sk-ant-spike-dummy-key-not-real`.
- Command: `claude -p --settings ... "Reply with exactly: OK"`.

No quota could burn: the key is dummy, every attempt fails closed at
authentication. Scratch files live under `/tmp` only.

## Observations

1. The helper IS invoked in `-p` mode: 10 calls before the run was
   stopped. Each call: `argc=0`, empty argv — the CLI passes no arguments.
2. The helper inherits the CLI's full environment (pass-through): the log
   shows `HOME`, `PATH`, `PWD`, CLI-set vars — and our own
   `RUNLITH_SPIKE_LOG`, proving arbitrary parent vars reach the child.
3. On auth failure the CLI retry-loops, re-executing the helper every
   attempt (10 invocations in ~2.5 minutes, still going when stopped).
   The run never exited on its own; it was terminated deliberately.

## Conclusions

- `apiKeyHelper` removes the token from the CLI's environment block, but
  the token still crosses into the sandbox via helper stdout → CLI
  memory. The helper itself is a sandboxed same-user child with the full
  parent env. Net exposure reduction: marginal, not architectural.
- Any adopted helper must be cheap and non-interactive: the retry loop
  hammers it (no vault-unlock prompts, no slow IPC per call; cache or
  fail fast).
- If pursued, the helper must fetch the token over a daemon channel
  (socket path in env = handle, not secret), never from env content.
  Even then, same-user visibility of helper output stays an accepted
  residual of the same class as Phase 1.

## Recommendation

Keep ADR-0040's ordering: runner destination pinning first (provider-
neutral, real egress reduction). Adopt `apiKeyHelper` delivery only as a
follow-up if hiding the token specifically from the CLI's environ block
is judged worth a socket-fed helper plus its retry-loop hardening.
