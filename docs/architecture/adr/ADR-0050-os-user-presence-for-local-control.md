# ADR-0050 - OS user presence for local execution and shutdown

Status: Accepted by the owner, 2026-11-04, selecting OS confirmation over
a same-UID-readable daemon capability. Implementation and verification
remain in progress; this status records the decision, not completion.

## Context

ADR-0020 authenticates a local OS user and deliberately does not attest a
genuine client or human presence. A 0600 capability file cannot exclude
malicious code with that UID. The owner requires OS confirmation for
Shutdown and starting/resuming execution, including changes to headless
CLI and unattended routines.

## Decision

Keep OS peer authentication on every connection. Additionally, the daemon
itself obtains a fresh OS user-presence result for each Shutdown and
execution Run/Resume request before their authoritative mutation or
process launch. The immutable request stays owned by that dispatch while
confirmation runs; no client boolean, PID, argv token or persistent file
can stand in for the OS result. The reason identifies the operation and
its target; client-supplied control characters are escaped.

On macOS use a fresh LocalAuthentication LAContext with
LAPolicyDeviceOwnerAuthentication: Touch ID or the account's system
password, entered only into the OS dialog. Set biometric reuse duration
to zero. Do not cache a successful confirmation or store a reusable grant.
Bound evaluation to 30 seconds, invalidate the context on completion or
timeout, serialize prompts and back off five seconds after refusal.
Run confirmation on the blocking pool, keeping control traffic responsive.

A platform without this implementation, an unavailable interactive OS
session, refusal, cancellation, timeout or internal failure denies the
operation. There is no same-UID, environment, CLI flag or headless fallback.
Read-only queries remain available under the OS peer contract. The CLI's protected-operation reply deadlines allow the bounded
OS dialog, and refusal is an error rather than a Shutdown acknowledgement.

Amendment (2026-11-05, owner decision after security review): the same
confirmation also gates execution Cancel, planning Trigger and
ApplyHumanContinuation, and widget draft Start, because each stops running
work or starts provider work. Status/Get/Cancel-of-a-widget-draft reads stay
available under the OS peer contract. The prompt names only validated,
bounded and escaped identities from the request (task, execution or decision
id; the first 80 characters of a widget description). For a Run, the
daemon also reads the task itself and shows its objective (first 80
characters), risk and scope size; after confirmation it re-reads the task and
refuses with a typed Conflict if objective, state, scope, acceptance, risk or
change intent differ from what the dialog showed. Resume and Cancel name
only the execution id.

Unattended routine ticks cannot authorize execution or trigger recurring
OS prompts. They record a named BLOCKED result without consuming the run
budget. An explicit routine authorization/run must also pass OS
confirmation; this does not create an unattended reusable grant.

Internal candidate helpers additionally require a private, per-launch
one-shot permit issued inside the verified Runner invocation. Bind the
random nonce to argv and the input digest, with a 30-second deadline.
Before candidate file access, macOS must authenticate the inherited socket
peer as the current parent with the same valid code-directory hash, using
its kernel audit token; caller-created tokens and other executable images
refuse. Only internal helper profiles enable the read-only
`process-info-codesignature` query needed for this check; provider/tool and
candidate Git profiles do not receive it. No process memory or attachment
authority is granted. The child consumes the permit over this socket exactly once.
Unsupported platforms refuse. This channel needs no public IPC mint route,
token file, persistent registry or shutdown listener.

## Alternatives

- Per-daemon file capability: rejected for the requested stronger boundary;
  arbitrary same-user code could read it.
- Client-supplied confirmation or authentication context: rejected; the
  daemon must observe the OS result itself.
- Cached/session-wide approval: rejected for this phase; introduces replay,
  expiry and unattended authority absent from the selected requirement.

## Consequences and security impact

Automation that previously issued privileged IPC silently now refuses when
OS confirmation is unavailable. Routine authorization no longer implies
permission for unattended execution. No account password enters Runlith,
the IPC, providers, plans, logs or SQLite. G6 policy, approvals, mandatory
Runner isolation and independent verification remain required after user
presence; OS confirmation does not authorize a wider command or grant.

This protects the daemon's exposed control path, not a modified daemon,
injected process, hostile OS or arbitrary filesystem access by the same
user. The helper fence verifies the running parent image, not publisher or
installation authenticity. Hardened runtime/signing and injection
resistance remain separate required work. A direct helper launch has no
exemption from policy or confinement requirements.

## Migration and verification

No data migration or secret file is needed. Optional refusal fields and
typed error values evolve the IPC without granting old clients new
authority; an old client may fail closed with a generic refusal.

Required evidence: denied/unavailable/timeout confirmation causes no
Shutdown, execution entry or provider spawn; success authorizes only its
one request; later requests confirm again; ping remains responsive during
confirmation; headless/native refusal; blocked ticks consume no budget;
updated shipped clients report denial honestly. A real OS success needs
an operator-attended smoke and is not established by a fake authenticator.

Positive process matrices use a separate Cargo example with explicit
test-support composition, installed beside the shipped CLI in a private
test directory. This preserves actual launcher, IPC, restart, G6 and
Runner checks without interactive prompts in automated tests. The
fixture requires an installation marker and is not the shipped daemon.
Native refusal and OS-dialog success remain separate evidence.
