# ADR-0046 — G12 Remote Worker contract

- Status: **Accepted** (TASK-0198; G12 plan ratified by owner `go`)
- Date: 2026-10-18
- Gate: G12
- Extends: source-of-truth §82 (workers make outbound authenticated
  connections; never open public agent ports; device keypair identity)

## Problem

G12 lets a second machine execute authorized work. That is the first
cross-device trust boundary in the product: device identity, enrollment,
authentication, job authorization and result evidence all need a frozen
contract, or every layer invents its own trust rule. A name string is
not an identity, and a result without authentication is not evidence.

## Decision

### 1. Device identity (W-1)

Each worker generates an ed25519 keypair on-device. The private key is
stored only through the existing vault/secrets layer and never crosses
the wire, logs or IPC payloads. Device identity is the public key:

| field | contract |
|---|---|
| `id` | `dev_` + 26-char Crockford Base32 |
| `name` | bounded text |
| `public_key` | 32-byte ed25519 public key (hex on the wire) |
| `fingerprint` | `sha256:<hex>` of the public key bytes |
| `state` | `ENROLLED` or `REVOKED` |
| `enrolled_by` | operator ref; `enrolled_at` Unix millis |

### 2. Authentication (W-2, W-3)

Challenge-response: the coordinator issues a random 32-byte challenge;
the worker signs `challenge || job_id`; the coordinator verifies with
the enrolled public key before accepting any claim or result. A bad
signature, an unknown device, or a revoked device refuses typed.
Challenges are single-use: a claimed challenge can never be replayed
to accept a second result (W-4).

### 3. Jobs (W-5..W-8)

| field | contract |
|---|---|
| `id` | `job_` + 26-char Crockford Base32 |
| `objective` | bounded text; the authorized work statement |
| `capability_scope` | ADR-0045 enum (`READ_ONLY`, `WORKSPACE_WRITE`, `EXECUTION`) |
| `authorized_by` | operator ref; jobs without it never dispatch |
| `status` | `OFFERED`, `CLAIMED`, `COMPLETED`, `FAILED`, `BLOCKED` |
| `claimed_by` | optional DeviceId of the claiming worker |
| `evidence_ref` | optional bounded text on completion |

Dispatch eligibility: device `ENROLLED` and job `OFFERED` with an
authorizer (W-5). A worker that needs new approval mid-job returns
`BLOCKED`; it never self-approves (W-6). One job, one claim: a second
claim refuses (W-7). Results are accepted only from the claiming
device, with a fresh signed challenge (W-8).

### 4. Outbound-only rule

The worker initiates connections; the coordinator never dials a
worker. In v1 both sides speak through the existing framed IPC
transport at the coordinator: the worker connects outbound to the
coordinator endpoint. No worker listens publicly. The coordinator's
existing endpoint ownership (single instance, peer auth) is unchanged.

### 5. Storage and IPC

SQLite tables `worker_devices`, `worker_jobs`, `worker_challenges`
(schema v36). Additive IPC V11, `worker_core = 14`; below V11 typed
refusal. Job execution on the worker still passes through existing
Policy/execution ports — transport grants no extra authority (W-9).
Revocation is durable and immediate: a revoked device's challenges
and results refuse from that moment (W-10).

## Alternatives considered

- Shared-secret HMAC identity: rejected; a leaked secret impersonates
  forever and cannot be enrolled by fingerprint.
- SSH tunnel transport: deferred; the signed-challenge contract is
  transport-independent and an SSH adapter can carry it later.
- mTLS with a local CA: deferred; enrollment by public-key fingerprint
  gives the same authentication property without operating a CA in v1.

## Consequences

A second machine can do authorized work with evidence the coordinator
can verify offline against enrolled fingerprints. Compromise of one
worker never grants another device's identity; revocation is one
durable transition.

## Security impact

First cross-device surface. Private keys never leave the device.
Authentication is signature verification in the trusted service, not
a prompt or a name. Replay is bounded by single-use challenges.
Residual risk: the coordinator endpoint becomes reachable by workers;
peer authentication and the signed challenge are both required, and
job authorization stays an operator act.

## Migration impact

Additive: schema v36 adds three tables; IPC V11 adds one envelope.
G13 (cloud/teams) builds on this identity; nothing here assumes cloud.
