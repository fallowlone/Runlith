# ADR-0037 Ratification Review Pack (TASK-0152)

> Independent architecture/security review input. Status of ADR-0037 stays
> **Proposed** until the owner ratifies after a PASS on this pack. Content
> under review: owner draft branch `origin/task-0152-confined-provider-auth-adr`
> (head `3924bb7`) merged into `task-0152-provider-auth-adr` (`9a62e3f`;
> INDEX-only conflict resolution, no content edits). Prior cycles: first
> review (B1-B3 blocking, N1-N10 non-blocking, remediated), second review
> (architecture/security PASS_WITH_NONBLOCKING_FINDINGS, adversarial FAIL,
> remediated in `7584e5b`/`3924bb7`). This pack is the third, currency +
> soundness pass.

## 1. What is claimed

- Option B (trusted-host authenticated loopback credential gateway,
  in-process in `agentd`) is selected; Option A (env-injected real
  credential) is rejected on the measured Seatbelt `*:443` host-confinement
  gap; Option C (in-sandbox Keychain grant) stays rejected per TASK-0112.
- The normative shape is D1-D27: per-attempt placeholder auth, pinned
  upstream, no-redirect, allowlisted request/header/body surface (D4/D15/
  D24), enumerate-and-bind every local address with loopback-peer filter
  (D11), Secrets Broker release to the gateway only (D12), fail-closed
  startup refusal (D14), TLS/provisioning/audit hardening (D16-D22, D25-D27).
- Residuals are explicit, not fixed: mid-attempt interface changes (D11a),
  non-loopback listener exposure + firewall prompt (D11b), same-user
  placeholder race (no TOCTOU analysis), other-uid discovery + bounded DoS
  (D27 ADR-0020 exception), Anthropic-server-mediated egress via tools/MCP
  (owner-accepted 2026-09-26), untested OAuth refresh, D24 not bounding
  actual spend.
- Docs-only: no code/schema/journal/IPC change; implementation follow-ups
  are proposed, not created; doc amendments carry fixed text applied by a
  later mechanical follow-up.

## 2. Reviewer checklist

- [ ] **Both task-envelope candidates evaluated.** Option A (broker injects
      into child env) and Option B (authenticated proxy/gateway) each get a
      full Mechanism/Threat-model/Lifetime/Audit/Failure/UX/Blast-radius/
      Compatibility/Disposition workup; Option C retained only because the
      envelope requires it. Keychain grant stays rejected.
- [ ] **Option A rejection is measured, not asserted.**
      `adapters/runner-macos-seatbelt/src/lib.rs:236-238` compiles
      `(remote tcp "*:443")` — port, not host allowlist; no per-host literal
      exists in the Runner/Seatbelt model.
- [ ] **Owner real-credential receipts exist and are bounded.**
      `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:874-923`
      records one short single-turn `200` per mode via the pre-round-4
      script; the ADR states what this does not prove (no long session,
      no refresh, round-4 hardening verified only via `--dry-self-check`).
- [ ] **No secret values in the adopted tree.** The receipts section
      records no token/account/org value; the exposure is described, and
      the token treated as exposed-until-revocation-confirmed. Long-token
      scan of the adopted diff shows only SHAs of scripts/commits.
- [ ] **Amendment anchors current.** `providers.md` §7 `:162-171`, §9a
      anchor sentence `:279-280`, H7 closure `:120-121`,
      `docs/privacy/PRIVACY_ARCHITECTURE.md:141-144`, ADR-0020 §7
      `:151-156` — all verified verbatim on the adoption base.
- [ ] **Code citations current.** `bins/agentd/src/execution.rs:88-89`
      (refusal const), `:98-111` (verdict enum), `:159-165` (macOS
      verdict), `:816-826` (refusal call site); `crates/runner/src/lib.rs`
      `:27`, `:34-64`; seatbelt `:103-248`, `:229-245`; provider-claude
      `environment.rs:142-199`, `authentication.rs:58-60`, `:94`.
- [ ] **Probe symbols exist at cited lines.** `owner_proxy_probe.py`
      `:274-296`, `:336`, `:382`, `:417-431`, `:462-471`, `:573-602`,
      `:652-659`, `:684-758`, `:800-807`, `:1210-1281`, `:1472-1489`;
      `provision_keychain_item.py` `:40-43`, `:119-133`, `:150-152`,
      `:193-222`, `:354-373`.
- [ ] **Non-blocking note (currency).** D25 cites
      `adapters/provider-claude/src/environment.rs:219-230` for the
      support-read roots; they now live at `:232-236`
      (`isolation_support_read_roots`, same `.claude`/`.claude.json`
      content). Content accurate, lines drifted; no ADR edit required
      (line pins are evidence pointers, and the follow-up reads current
      code).
- [ ] **No implementation authorized.** "Authorizes no production
      implementation on its own"; follow-ups proposed-not-created;
      "Removal of the unconditional refusal" gated on a future green
      real-provider smoke. TASK-0114 refusal stays exact behavior.
- [ ] **AGENT.md §11 shape.** Problem/context, decision (Option B +
      D1-D27), alternatives (A rejected, C rejected, separate-process
      recorded-not-ratified), consequences, security impact (residuals +
      cannot-prevent list), migration impact (docs-only, additive, trivial
      rollback) — all present.

## 3. Verdict

**PASS** — adopt and ratify. One non-blocking currency note above; no
blocking finding. Ratification proceeds under the owner's standing
G8-closure directive; the adopted content is the owner's own draft
branch including owner-recorded decisions (Option B selection 2026-09-25,
compliance risk 2026-09-25, egress + D27 exception 2026-09-26) and
owner-run receipts.

## 4. For the owner

- Revocation of the exposed OAuth token (2026-09-25 exposure) remains
  unconfirmed; the ADR correctly treats it as exposed, not revoked.
- The round-4 hardened gateway was never exercised against a real
  credential end-to-end (only `--dry-self-check` rejection paths); the
  first real-credential run of the production Rust gateway is still
  ahead in the implementation follow-ups.
- The `#9` sibling branch (credential-input probe scripts) is
  intentionally not adopted: nothing in the ADR or evidence doc
  references those four scripts.
