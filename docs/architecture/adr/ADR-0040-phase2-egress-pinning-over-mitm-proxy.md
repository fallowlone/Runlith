# ADR-0040 — Phase 2 Credential Egress: Runner Destination Pinning over MITM Proxy

Status: **Proposed** (2026-10-05; awaits owner ratification. ADR-0039
explicitly deferred Phase 2 to its own decision.)

Gate: G6 follow-up (provider authentication under mandatory confinement)

> Phase 1 (ADR-0039, brokered credential release into the sandbox) stands.
> The remaining Phase 2 goal — the provider credential never entering the
> sandbox — is pursued with runner-level destination pinning on `tcp:443`
> plus a bounded `apiKeyHelper` delivery spike, instead of a
> credential-injecting MITM proxy with a locally installed CA.

## Context

ADR-0039 Phase 1 releases the vault credential into the confined child
environment at launch. The accepted residual is same-user visibility of
the child environment (ADR-0039 Security impact). Phase 2 was sketched as
a credential-injecting local proxy with hostname pinning (A4), deferred
because "only `tcp:443` is supported" and it "needs a runner change".

Measured facts since (2026-10-05, CLI as installed):

- The Claude Code CLI advertises no `--proxy` / `--base-url` flag and no
  `HTTP(S)_PROXY` support in `--help`. A plain proxy env cannot inject a
  header into the CLI's own TLS session.
- The CLI's auth inputs are: OAuth/keychain login, `ANTHROPIC_API_KEY`,
  or `apiKeyHelper` via `--settings` (the CLI executes a helper to obtain
  the key material).
- The Seatbelt backend already scopes egress to `(remote tcp "*:443")`
  (`adapters/runner-macos-seatbelt/src/lib.rs`); literals are supported,
  so address restriction needs no profile widening.

## Decision

1. **No MITM proxy, no locally installed CA.** Rejected (see Alternatives).
2. **Runner destination pinning.** Resolve the provider API host at
   profile-build time and restrict the sandbox rule from `"*:443"` to the
   resolved addresses, re-resolved on each launch (DNS may move). This is
   provider-neutral infrastructure: the second provider reuses the same
   mechanism with its own host set.
3. **Bounded `apiKeyHelper` spike (measurement only).** Test whether
   serving the vault credential through a `--settings` `apiKeyHelper`
   removes it from the CLI's environment block, and measure what exposure
   remains (the helper is still a sandboxed child process). Adopt only if
   the measurement shows a real reduction; the spike itself changes no
   production path.

## Alternatives considered

- **MITM proxy with local CA (rejected).** The only design that fully
  keeps the credential out of the sandbox with the current CLI, but: it
  installs a trust root into the system store, adds bespoke TLS
  interception code (high-risk, permanent maintenance), and is
  Claude-specific throwaway work under provider neutrality (§13). Against
  the documented same-user threat model — where a local CA key would live
  on the same machine — it does not raise the bar over Phase 1. Adopt
  only if the threat model escalates beyond same-user.
- **Keep the Phase 1 residual with no further work (fallback).**
  Acceptable: the residual is already owner-accepted in ADR-0039, and the
  P6a/P6d adversarial measurements cover the current path. Taken if
  pinning proves infeasible.
- **Plain `HTTPS_PROXY` env (rejected).** Cannot inject credentials into
  the CLI's TLS; captures CONNECT hostnames at best. No credential
  benefit.

## Consequences

- Runner gains an address-scoped egress form alongside the current
  `tcp:443`-to-anywhere scope; the Seatbelt profile is narrowed, never
  widened (hard constraint, unchanged).
- DNS moves: pinning must re-resolve per launch; a stale pin fails closed
  (connection refused) rather than falling back to `"*:443"`.
- No new persistent state, no SQLite schema change, no IPC contract
  change for pinning. The `apiKeyHelper` spike, if adopted, needs a
  settings-plumbing decision of its own.

## Security impact

- Pinning shrinks exfiltration surface (arbitrary `*:443` → provider API
  addresses) without new trusted components or trust-store changes.
- The credential-in-sandbox residual stays accepted until and unless the
  spike shows a real reduction; `SecretAccess` / `ProviderInvocation`
  audit coverage is unchanged, values stay out of logs and receipts.
- Failure mode is fail-closed on both paths (refused connection;
  refused launch).

## Migration impact

- None for existing vaults, databases, or IPC peers. Sandbox rules get
  stricter; a launch that previously reached a non-pinned host now
  fails with the existing network-denial cause.

## Addendum — IP pinning unimplementable on Seatbelt (2026-10-06)

Measured against the system `/usr/bin/sandbox-exec` on macOS: a profile
containing `(remote tcp "93.184.216.34:443")` is rejected at parse time
with `sandbox-exec: host must be * or localhost in network address`
(exit 65), while `localhost:443` and `*:443` parse and run (exit 0).
Minimal three-case probe; see
`seatbelt_remote_tcp_accepts_only_star_or_localhost` in
`adapters/runner-macos-seatbelt/src/lib.rs`, which pins this platform
fact and fails loudly if Apple ever relaxes it.

Consequences for this ADR:

- Per-IP destination pinning in the Seatbelt profile is not available on
  the only supported platform. The pinning implementation was reverted;
  no `tcp:443:<ip>` scope, resolver, or render code was kept.
- The `localhost` form IS expressible, so a localhost-bound egress
  filter remains theoretically possible — but only behind a local proxy
  the CLI can be pointed at, which re-opens the Phase 2 proxy questions
  (TLS interception or plaintext loopback the CLI does not advertise).
  That is new work needing its own spike, not a continuation of this one.
- Standing recommendation until such a spike lands: keep ADR-0039
  Phase 1 (brokered release, adversarially measured) with the accepted
  same-user residual; do not re-attempt IP pinning on Seatbelt.
