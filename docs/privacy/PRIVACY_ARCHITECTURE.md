# Privacy Architecture

> Technical privacy architecture of AI Engineering OS: what stays local,
> what may leave the device, who receives it, and under which user-visible
> contract. Architectural — no legal prose, no implementation.
>
> - Stable guarantees: [`TECHNICAL_PRIVACY_CONTRACT.md`](./TECHNICAL_PRIVACY_CONTRACT.md)
> - Flow registry: [`DATA_FLOW_MAP.md`](./DATA_FLOW_MAP.md)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Threat scenarios: [`../security/THREAT_MODEL.md`](../security/THREAT_MODEL.md)
> - Boundaries: [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md)
> - Decision: [`../architecture/adr/ADR-0014-local-first-privacy.md`](../architecture/adr/ADR-0014-local-first-privacy.md)

Core promise: **local-first + cloud-optional**. The core product remains
fully usable without our cloud, an account, telemetry or cloud sync
(PRIV-001, PRIV-002, PRIV-009). Local-first does **not** mean no data ever
leaves the device: when the user chooses an external AI provider, selected
context goes to that provider for inference. That flow is explicit,
observable, and distinct from sending anything to our infrastructure.

Privacy is not security restated. Security asks: can an actor access or do
something? Privacy asks: where does data go, why, and under what
user-visible contract? A secure HTTPS transfer to an external provider is
still a privacy-significant transfer. Likewise, Data Classification
(sensitivity/handling category) is not Privacy Architecture (allowed flows,
ownership, user-facing guarantees) — the two are referenced together, never
collapsed.

## 1. Local-by-default baseline

Canonical application-owned state is local by default:

```text
Repository            LOCAL
Task history          LOCAL
Memory                LOCAL
Agent transcript      LOCAL
Git                   LOCAL
Analytics             LOCAL
Telemetry             OFF
```

LOCAL means the canonical application-owned copy lives on the user's device.
It does not prevent an explicitly selected external AI provider from
receiving the minimal execution context required for inference — that is an
explicit, observable provider flow (FLOW-003), not a violation of LOCAL.

## 2. What may cross the device boundary

Only these categories of transfer are architecturally accepted:

- **Selected provider-bound context → external AI provider** (FLOW-003):
  the smallest data set required for the authorized execution, chosen by
  Context selection under classification and policy rules.
- **Provider response → local runtime** (FLOW-004): provider output returns
  as untrusted data.
- **Browser / Web Acquisition → general internet** (FLOW-009): fetches and
  browser sessions, SSRF-policed, separate from provider inference.
- **External document vision inference, when enabled** (FLOW-010):
  document/page representations to a vision provider; deferred, must be
  observable at page/representation/provider level.
- **Optional cloud sync → our infrastructure** (FLOW-007): opt-in only,
  implementation deferred.
- **Optional telemetry → our telemetry endpoint** (FLOW-008): off by
  default, prohibited content excluded by construction (PRIV-006).
- **Scoped plugin/integration transfers** (FLOW-012): explicit
  capability/scope only.
- **Future remote worker** (FLOW-011): conceptual distinction reserved,
  implementation deferred.

Everything else stays on-device. Indexing or parsing repository content
(FLOW-001) and local-model inference (FLOW-002) cause no external transfer.

## 3. Our infrastructure vs the model provider

The fundamental distinction:

```text
Local App
 |
 +---- task metadata ------------> remains local
 |
 +---- selected context ---------> Claude Code
 |
 v
 Anthropic
```

This means Anthropic received selected context. It does **not** mean AI
Engineering OS cloud received the context. The same holds for Codex →
OpenAI, Gemini → Google, and any future provider → provider backend. These
vendors are illustrative provider flows only — never domain semantics
(provider neutrality: INV-012, INV-059).

Destination vocabulary is therefore never a generic "cloud". Flows name one
of: AI provider infrastructure; AI Engineering OS infrastructure;
third-party plugin/integration; general internet/web target; future remote
worker.

## 4. What our own infrastructure receives

By default: nothing. With no optional feature enabled, zero bytes of user
data reach AI Engineering OS servers (PRIV-003). Our infrastructure receives
data only through explicitly enabled flows:

- opt-in cloud sync payloads (FLOW-007), inspectable before upload;
- opt-in telemetry payloads without prohibited content (FLOW-008);
- future account/cloud-control-plane interactions, not defined here.

Provider-bound context never passes through our servers on its way to a
provider: a local CLI/API transfer to Anthropic/OpenAI/Google is not our
cloud receiving the data.

## 5. Provider-owned credentials

Provider-native authentication remains provider-owned whenever possible. If
a provider CLI (Claude Code, Codex, etc.) is already authenticated, AI
Engineering OS does not scrape or extract provider credentials to reuse the
session. Actual secrets never enter model context; when a tool needs a
secret, the path is:

```text
secret reference
 ↓
Policy / effective capability
 ↓
Secrets Broker
 ↓
trusted execution boundary
```

Release also requires the exact bound execution/tool target; provider-native
login state cannot grant access. Secret bytes are borrowed only at the trusted
delivery boundary and never become ordinary application state or a prompt.
TASK-0064 runner isolation plans likewise contain no secret payload: they carry
only canonical capability scope and required enforcement axes. Provider
environment construction and HOME/XDG/state/scratch isolation remain
TASK-0065 rather than broadening this data flow.
Provider-owned session copies (e.g. a provider retaining
transcript data on its side of an external execution) follow that
provider's execution path and terms — outside our application's control and
never presented as ours (PRIV-004).

## 6. Local, transient, derived

- **Local durable:** app-owned categories in §9; canonical state owned per
  [`ARCHITECTURE.md`](../../ARCHITECTURE.md) §5; user-inspectable and
  deletable (PRIV-007, PRIV-008).
- **Ephemeral execution data:** provider-bound context packages, in-flight
  tool envelopes, rendered model-facing views — derived, disposable
  (INV-016), never authoritative, discarded with the execution unless
  explicitly persisted as evidence.
- **Derived representations:** summaries, embeddings, Markdown/OCR outputs,
  context plans. Derivation never lowers sensitivity by itself: a summary of
  SECRET is not PROJECT, an embedding of PRIVATE is not PUBLIC
  (conservative inheritance, [`DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)).

## 7. Data minimization

External transfer carries the smallest data set required for the authorized
capability. The Context Engine prefers selected relevant evidence over whole
repository dump (INV-115: whole-repository packing is an explicit bounded
fallback, structural just-in-time retrieval is the default). Minimization is
an architectural principle tied to Context selection and Data
Classification — not a hard algorithm; this task defines no thresholds.

## 8. Provider boundary observability

Every external-provider execution is conceptually separable into: local
orchestration → local durable state → local Context compilation →
provider-bound payload → provider inference → provider response → local
persistence after response. The system must eventually explain, per
execution: provider, model, what representation/data was sent, data
classification, approximate/known quantity, whether data left the device,
whether our infrastructure received it, and execution time/reference
(INV-084 preflight; FLOW-003 observability). No database tables are defined
here.

## 9. Local data ownership

App-owned local data categories (conceptual, no storage paths or schemas):

```text
Task state
Execution/Attempt metadata
Event Journal
Memory
Resource registry / derived artifacts
Context checkpoints / history references
Agent transcripts
Operational logs
Security Audit
Usage ledger
Configuration
Generated documentation / cache / projections
```

## 10. Inspectability and deletion

PRIV-007: users can inspect locally stored app-owned data; no category may
be deliberately hidden behind opaque storage without a product
inspection/export path (final UI unspecified). PRIV-008: users can delete
all app-owned local data. Deletion limits are explicit: this removes
app-owned local data only — not provider-side records, Git remotes,
third-party cloud data, or OS-wide files the app does not own — unless an
integration explicitly supports that capability.

## 11. Transcripts

Agent Transcript is a distinct user-controlled category — not Operational
Log, Security Audit, Event Journal or Memory. It is local by default, never
automatically promoted to Memory, never automatically uploaded to our
cloud. Provider-native copies may exist at the external provider according
to that provider's execution path. Exact retention controls remain
implementation/privacy-policy work unless already specified.

## 12. Memory privacy

Memory is local by default. Promotion into Memory never grants
external-transfer permission: when memory contributes to an external
provider context, ordinary Context, Data Classification and Policy rules
still apply. Stored in Memory never means safe to send anywhere.

## 13. Resources and imported data

Repository, document, web and resource data preserve classification,
provenance and trust as independent dimensions. Derived representations do
not automatically become less sensitive: a PRIVATE document converted to
Markdown stays appropriately protected; a RESTRICTED source's embedding does
not become PUBLIC. Full rules live in
[`DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md).

## 14. Provider privacy labels

Conceptual per-provider UI/data contract (not implemented here; provider
behavior differs per provider):

```text
Provider: Claude Code
Execution: Local
Inference: External - Anthropic
Memory: Local
Telemetry: Off
Data leaves device: YES
Our cloud receives execution payload: NO
```

Local-model case:

```text
Execution: Local
Inference: Local
Memory: Local
Data leaves device: NO
```

## 15. Data Flow Inspector

Future observable privacy capability — an observability requirement, not an
analytics pipeline. Conceptual output:

```text
Execution EXEC-...

Sent to external AI provider:
14 files
31 KB

Sent to AI Engineering OS servers:
0 bytes

Local memory writes:
3

Artifacts created locally:
2
```

The architecture must eventually preserve enough provenance to produce this
from real execution evidence. No fake counters or tracking implementation
are created now. Before any documentation-cloud upload, the Inspector must
show which artifacts/resources leave the machine (INV-058 context).

## 16. Analytics vs telemetry; crash reports

Analytics is LOCAL; telemetry is OFF. A local Usage Ledger or performance
statistics never imply external upload. Crash-report collection and transfer
policy is unresolved — no automatic crash uploading is assumed; the default
must not contradict telemetry-off-by-default:

```text
OPEN PRIVACY DESIGN ITEM:
crash-report collection and transfer policy
```

## 17. Cloud sync and remote workers

Cloud sync is opt-in; core never requires it. No sync protocol, encryption
implementation, storage vendor or retention duration is defined here — sync
is a future explicit flow with observable boundaries. Remote execution is a
later feature: the flow map reserves `local device ↔ authorized remote
worker` as distinct from AI provider, our cloud, plugin and general web,
with implementation deferred.

## 18. Plugins and integrations

Plugins/integrations never inherit ambient access to project data. Accepted
privacy requirements: explicit capability/scope, data-flow observability,
least necessary transfer. The full permission and data-sharing model is not
defined here:

```text
OPEN PRIVACY DESIGN ITEM:
plugin permission / data-sharing model
```

## 19. Architecture privacy contracts

The binding guarantees are [`TECHNICAL_PRIVACY_CONTRACT.md`](./TECHNICAL_PRIVACY_CONTRACT.md)
(PRIV-001…PRIV-010), grounded in INV-008, INV-011, INV-014, INV-015,
INV-058, INV-084, INV-130 and ADR-0014.

## 20. Open Privacy Design Items

Unresolved by accepted architecture; listed, not decided. Each is included
only because no accepted source resolves it:

- data-at-rest encryption policy;
- app-owned data export format/scope;
- local retention/cleanup policy by data category;
- future cloud retention policy;
- crash-report collection and transfer policy;
- telemetry schema/minimization;
- plugin permission/data-sharing model;
- remote-worker privacy contract;
- provider privacy metadata/catalog model;
- cloud sync encrypted-data model;
- legal subprocessor mapping.

No retention durations, export formats, schemas or protocols are invented
here.

## 21. Legal boundary

```text
TECHNICAL PRIVACY CONTRACT
!=
LEGAL PRIVACY POLICY
```

No `legal PRIVACY_POLICY.md` is created by this task. Before public launch,
legal review must address account data, local data, AI providers, cloud
sync, encryption, telemetry, crash reports, payments, subprocessors,
retention, export, deletion, GDPR rights, international transfers, training
usage and sale/ads policy. Answers are not drafted here where the technical
architecture has not established them.
