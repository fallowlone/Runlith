# Technical Privacy Contract

> Stable technical privacy guarantees of AI Engineering OS. IDs `PRIV-001`
> through `PRIV-010` are fixed — preserved exactly as accepted, never
> renumbered, never silently reinterpreted.
>
> - Architecture: [`PRIVACY_ARCHITECTURE.md`](./PRIVACY_ARCHITECTURE.md)
> - Flow registry: [`DATA_FLOW_MAP.md`](./DATA_FLOW_MAP.md)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Decision: [`../architecture/adr/ADR-0014-local-first-privacy.md`](../architecture/adr/ADR-0014-local-first-privacy.md)

```text
TECHNICAL PRIVACY CONTRACT
!=
LEGAL PRIVACY POLICY
```

This file states what the technical architecture guarantees. It is the basis
for a future legal Privacy Policy, not a substitute for one. Before public
launch, legal review must address account data, local data, AI providers,
cloud sync, encryption, telemetry, crash reports, payments, subprocessors,
retention, export, deletion, GDPR rights, international transfers, training
usage and sale/ads policy — but no answers to those legal topics are drafted
here where the architecture has not established them. A future legal policy
must reflect actual implementation; implementation must never be changed
merely to make marketing or privacy copy easier.

## PRIV-001

- **ID:** PRIV-001
- **Guarantee:** Core product works without an account.
- **Technical meaning:** the full core loop (task, execution, context,
  memory, verification) runs on the local device with no account, no login,
  no cloud identity. Local architecture must assume no network and no
  account (ADR-0014).
- **Applies to:** core product functionality.
- **Does not imply:** that optional features (cloud sync, future team
  features, provider CLIs with their own logins) need no identity of their
  own; provider-native authentication stays provider-owned.
- **Required implementation evidence:** core flows exercised with no account
  configured and no network (acceptance tests, not prose).
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-011; ADR-0014.

## PRIV-002

- **ID:** PRIV-002
- **Guarantee:** Core product works without telemetry.
- **Technical meaning:** core functionality never depends on telemetry;
  telemetry is off by default and opt-in. Analytics (local usage/cost
  measurements) is not telemetry: local analytics never implies upload.
- **Applies to:** core product functionality.
- **Does not imply:** that an explicitly enabled telemetry feature sends
  nothing (see PRIV-006 for what it still may never contain), nor that local
  usage statistics cannot exist on-device.
- **Required implementation evidence:** telemetry-off default verified;
  core-path tests pass with telemetry disabled and unreachable.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-014; ADR-0014.

## PRIV-003

- **ID:** PRIV-003
- **Guarantee:** Source code is not uploaded to our servers unless explicitly
  required by an enabled feature.
- **Technical meaning:** our infrastructure (AI Engineering OS servers)
  receives source code only through an explicitly enabled, user-visible
  feature (e.g. opt-in cloud sync), never as a side effect of local work,
  indexing, parsing or execution. Sending selected context to an external AI
  provider is a different flow and does not touch our servers (see
  [`DATA_FLOW_MAP.md`](./DATA_FLOW_MAP.md)).
- **Applies to:** AI Engineering OS infrastructure as a destination.
- **Does not imply:** that source never leaves the device — an explicitly
  selected external AI provider receives the minimal provider-bound context
  required for inference.
- **Required implementation evidence:** data-flow observability showing
  zero bytes to our servers for local executions; sync payloads inspectable
  before transfer.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** PRIV-009; INV-011; ADR-0014.

## PRIV-004

- **ID:** PRIV-004
- **Guarantee:** Customer repositories/prompts/outputs are not used by us to
  train models.
- **Technical meaning:** AI Engineering OS itself never uses customer
  repositories, prompts or outputs as model-training data.
- **Applies to:** our use of customer data.
- **Does not imply:** anything about external AI providers — provider-side
  data handling and training policies are governed by the provider's terms,
  not by our application. Product UI and documentation must distinguish our
  use from provider terms.
- **Required implementation evidence:** no training pipeline consuming
  customer data exists in our systems; provider distinction is user-visible.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** ADR-0014.

## PRIV-005

- **ID:** PRIV-005
- **Guarantee:** Personal data is not sold.
- **Technical meaning:** no product design depends on selling personal or
  project data. This is a constraint on the architecture, not a legal
  definition of personal data or sale.
- **Applies to:** product and architecture design.
- **Does not imply:** legal interpretations of data-sale law; those belong
  to the future legal Privacy Policy.
- **Required implementation evidence:** absence of any sale-dependent data
  pipeline or integration by architecture review.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** ADR-0014.

## PRIV-006

- **ID:** PRIV-006
- **Guarantee:** Telemetry contains no source code, prompts, model outputs,
  secrets or raw filesystem contents.
- **Technical meaning:** if telemetry is ever enabled, its payload excludes
  all five categories by construction — not by sampling, redaction luck or
  downstream filtering.
- **Applies to:** any current or future telemetry payload to our telemetry
  endpoint.
- **Does not imply:** that telemetry is safe to enable blindly; the
  telemetry schema and minimization design remain future implementation work.
- **Required implementation evidence:** telemetry-schema review plus payload
  tests proving the excluded categories cannot be present.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-008; classification SECRET/PROJECT/PRIVATE rules in
  [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md).

## PRIV-007

- **ID:** PRIV-007
- **Guarantee:** User can inspect locally stored application data.
- **Technical meaning:** no app-owned durable local state is deliberately
  hidden behind opaque, inaccessible storage without a product
  inspection/export path. Final UI is not specified here.
- **Applies to:** all app-owned local data categories (see
  [`PRIVACY_ARCHITECTURE.md`](./PRIVACY_ARCHITECTURE.md)).
- **Does not imply:** access to provider-side records, Git remotes,
  third-party cloud data, OS-wide files not owned by the app, or SECRET
  values (references/handles only).
- **Required implementation evidence:** an inspection path exists for each
  app-owned local category.
- **Status:** Accepted architecture contract — implementation pending.

## PRIV-008

- **ID:** PRIV-008
- **Guarantee:** User can delete all app-owned local data.
- **Technical meaning:** the user can remove every app-owned local data
  category, returning the device to a state with no application-owned
  project data.
- **Applies to:** app-owned local data only.
- **Does not imply:** deletion of provider-side records, Git remotes,
  third-party cloud data or OS-wide files not owned by the app, unless an
  integration explicitly supports that capability.
- **Required implementation evidence:** deletion path covering all app-owned
  local categories, verified by test.
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-015.

## PRIV-009

- **ID:** PRIV-009
- **Guarantee:** Cloud sync is opt-in.
- **Technical meaning:** no synchronization of application data to our
  infrastructure happens unless the user explicitly enables it. Sync is
  independent of local engines — local functionality never requires it.
  Sync protocol, encryption implementation, vendor and retention are not
  defined here (open design items).
- **Applies to:** any transfer of application data to AI Engineering OS
  infrastructure for synchronization purposes.
- **Does not imply:** that enabling sync uploads everything; future sync
  design must keep explicit, observable payload boundaries.
- **Required implementation evidence:** default-off verified; pre-upload
  inspection of sync payloads (Data Flow Inspector).
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-058; ADR-0014.

## PRIV-010

- **ID:** PRIV-010
- **Guarantee:** Provider data transfer is shown clearly.
- **Technical meaning:** when an external AI provider receives data, the
  system makes the transfer user-visible: which provider, what kind of
  data, and that the destination is the provider — not our infrastructure.
  Realized through provider privacy labels and the Data Flow Inspector
  concepts (see [`PRIVACY_ARCHITECTURE.md`](./PRIVACY_ARCHITECTURE.md)).
- **Applies to:** every external-provider execution.
- **Does not imply:** a fixed UI design or a catalog of provider policies;
  provider data behavior differs per provider and is not controlled by us.
- **Required implementation evidence:** execution-time transfer disclosure
  backed by real execution provenance (no fake counters).
- **Status:** Accepted architecture contract — implementation pending.
- **Refs:** INV-084; ADR-0014.
