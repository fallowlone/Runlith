# Study

Provider authentication boundary in agent orchestrators.

- study_type: cross-product architecture study (not a source-level repository audit)
- review_date: `2026-09-19`
- sources: vendor documentation and a published protocol specification, read live
- evidence_level: **documentation-level, not source-level**

# Method limitation, stated up front

The `repos/` re-audits in this directory pin an exact upstream commit and read
source. This study does not. Its sources are vendor documentation and one
protocol specification, read on the review date. Documentation can lag or
contradict the shipped implementation, and one source below is internally
inconsistent (see F-5).

Findings here are therefore **convergent design evidence**, not verified
implementation behaviour. Nothing in this study is an authority for Runlith,
and no finding is adoptable without an independent implementation check.

# Research target

Two questions, raised by the TASK-0102 investigation:

1. How do comparable agent orchestrators authenticate against a provider that
   is itself an agent process?
2. Can such orchestrators drive a provider that is **not** a raw model API —
   a subscription-authenticated CLI agent — and if so, where do they put the
   authentication boundary?

The immediate motivation: Runlith sandboxes the `claude` CLI and discovered
that provider-owned authentication is unreachable inside that sandbox
(TASK-0101, TASK-0102). The question is whether the industry solves this by
widening the sandbox, or by moving the boundary.

# Sources reviewed

| Source | What it is | Reviewed |
|---|---|---|
| Agent Client Protocol (`agentclientprotocol.com`) | Published protocol spec for editor/agent communication, v1 | 2026-09-19 |
| Zed external agents (`zed.dev/docs/ai/external-agents`) | ACP client implementation, vendor docs | 2026-09-19 |
| Cline (`docs.cline.bot`, Anthropic/Claude Code provider) | IDE extension, vendor docs | 2026-09-19 |
| OpenCode (`opencode.ai/docs/providers`) | Multi-provider agent, vendor docs | 2026-09-19 |
| ZCode (`docs.z.ai/devpack/tool/zcode`) | Z.ai first-party harness for GLM, vendor docs | 2026-09-19 |

# Findings

## F-1 — The orchestrator does not own provider authentication

Every source separates the two. Zed's external-agent documentation states that
the external agent owns its own runtime, authentication, model selection,
tools and native configuration, while Zed hosts only the thread. It further
notes that billing, legal terms, retention and data handling are between the
user and the agent provider, not Zed.

Zed makes the separation concrete: an Anthropic API key configured for Zed's
own agent does **not** configure the Claude external agent. The two credential
scopes are deliberately disjoint.

## F-2 — Authentication is a protocol-level state, not an exit code

ACP treats authentication as a first-class negotiated concern:

- the agent advertises `authMethods` in its `initialize` response;
- an authentication-gated request fails with a typed `auth_required` error;
- `agentCapabilities.auth.logout` advertises whether logout exists at all.

The orchestrator never handles a credential. It learns *that* authentication
is required, and *which* flows the agent offers.

This is the direct counterpart to the Runlith defect chain. Under the current
integration an unauthenticated provider surfaces as a bare exit 1 with empty
stderr — indistinguishable from an unrelated launch failure, and the reason
two gate closures recorded a green smoke over a provider that never once
succeeded (TASK-0101).

## F-3 — Interactive login is a separate launch, by design

ACP's `type: "terminal"` authentication method is the load-bearing find. Its
flow:

1. the client launches a **separate** interactive process using the same
   configured agent program and base launch configuration;
2. it appends the method's `args` and applies its `env`;
3. it presents the terminal to the user and waits for exit — zero is success;
4. it reconnects and reinitialises the ACP agent.

Three properties matter for Runlith:

- The descriptor cannot supply a command. The client derives the command from
  its own agent configuration, so an agent cannot name an arbitrary binary for
  the client to run. Fail-closed by construction.
- The terminal process **is not the ACP connection**. Login and work are two
  different launches with different requirements.
- The client advertises `clientCapabilities.auth.terminal` only when it can
  actually reproduce the invocation interactively; an agent may advertise a
  terminal method only when that capability is true. Neither side assumes.

ACP explicitly declines to define an in-band success signal, relying on exit
status. That is a deliberate narrowing, not an omission.

## F-4 — Subscription-authenticated CLI providers are a supported pattern

Both Cline and Zed drive Anthropic's own CLI under a user subscription as a
first-class provider option, alongside API-key access.

Cline documents two Anthropic paths: an API key, or Claude Code under a
Max/Pro subscription. The subscription path's first instruction is to install
**and authenticate** the CLI per Anthropic's own documentation; Cline then
asks only for the executable path. Cline never participates in the login.

Cline also documents the cost of the CLI path honestly: responses may not
stream token-by-token, and image upload and prompt caching are limited. The
subscription path is not strictly better — it trades capability for billing
model.

Zed's Claude agent exposes `/login` inside the thread and lets the user choose
an API key or Claude Code where supported. Again: the agent owns the flow, the
orchestrator surfaces it.

## F-5 — A compliance boundary exists here, and this study does not resolve it

OpenCode's provider documentation states that plugins allowing Claude Pro/Max
models to be used in OpenCode are prohibited by Anthropic, and that such
plugins were unbundled as of version 1.3.0. The same page also describes
selecting a Claude Pro/Max option through its `/connect` flow.

**The page is internally inconsistent, and this study does not interpret a
third party's account of another vendor's terms.**

The distinction that appears to matter, and that Runlith should verify against
primary terms rather than against this evidence:

- running **Anthropic's own CLI** under a user's subscription — what Runlith,
  Cline and Zed do — is use of the vendor's shipped product;
- reproducing subscription credentials to call the raw API directly is a
  different act, and is what the OpenCode documentation describes as
  prohibited.

This bears directly on the HTTP-adapter option discussed during TASK-0102. The
sketch floated there — reusing a file-based OAuth profile produced by an
interactive subscription login — sits on the wrong side of that line as
described. If an HTTP adapter is ever built, the safe credential is a
token-billed API key delivered through the existing Secret Broker, not a
repurposed subscription credential.

**Unresolved. Requires verification against Anthropic's primary terms before
any adapter work is scoped.**

## F-6 — Interactive login resolves to a non-interactive credential

ZCode's configuration flow converts a subscription into a key: the user may
enter an API key manually, or choose to use a subscription, which redirects to
a login page after which the key is populated automatically.

This is the same shape as F-3 reached by a different route. A human completes
an interactive flow once; the runtime afterwards holds an ordinary bearer
credential and never performs an interactive login itself.

## F-7 — The Anthropic wire format is becoming a third-party interop surface

ZCode documents Anthropic-compatible and OpenAI-compatible base URLs for its
own GLM coding plan, including an `…/api/anthropic` endpoint.

Relevant to ADR-0009 provider neutrality: an HTTP adapter speaking the
Anthropic Messages wire format is not necessarily an Anthropic lock-in, because
other vendors serve that format deliberately. This weakens one standing
objection to an HTTP adapter, without by itself justifying one.

# Convergent pattern

Across a published protocol and four independent products:

> The orchestrator never owns provider authentication. The agent process owns
> it. Authentication is a separate interactive phase with a human in the loop,
> and the orchestrator's job is to detect an unauthenticated state as a typed
> condition and route the human into the agent's own login flow.

No reviewed source sandboxes an interactive login. No reviewed source grants a
worker process platform-credential access so that it can log itself in.

# Runlith overlap search

| Runlith element | Relationship |
|---|---|
| `ProviderEnvironment` "provider-owned authentication" comment | The **principle** already exists and is stated in code. What is missing is the boundary that makes it true. |
| `crates/secrets` Secret Broker (TASK-0063) | Already the right home for a token-billed API credential. Not applicable to CLI subscription auth, which Runlith must not hold at all. |
| `isolation_support_read_roots` | Already the shape for file-based credentials. It is why F-6 is cheap for Runlith and Keychain is not. |
| `SupportService` (TASK-0102, uncommitted to main) | Correct mechanism, wrong first user. See adoption decision. |
| Typed provider failures (`PlannerFault`, `ExecutionProblem`) | The vocabulary for `auth_required` exists; the state does not. |

# Novelty classification

- F-1: `PARTIALLY_EXISTS` — the principle is stated in Runlith, the enforcement is absent.
- F-2: `NOVEL_GAP` — Runlith has no typed unauthenticated-provider state.
- F-3: `NOVEL_GAP` — Runlith has no separation between a login launch and a work launch.
- F-4: `EXTERNAL_VALIDATION` — Runlith's CLI-under-subscription choice is a mainstream pattern, not an oddity.
- F-5: `NEEDS_ADR` — and needs a primary-source legal check before that.
- F-6: `PARTIALLY_EXISTS` — Runlith can read file credentials already.
- F-7: `EXTERNAL_VALIDATION` — bears on ADR-0009 only if an HTTP adapter is proposed.

# Differentiation test

Does adopting F-2 and F-3 make Runlith more like its competitors, or better at
its own thesis?

Runlith's thesis is policy-governed, independently verified execution with a
real OS isolation boundary. Cline and Zed do not sandbox the provider at all,
which is precisely why they never met this defect. Runlith does — so it is the
only one of the five that **needs** the login/work split to be explicit,
because it is the only one where the work launch is genuinely confined.

Adopting the pattern therefore strengthens the differentiator rather than
diluting it: it lets the sandbox stay tight instead of being widened to
accommodate a phase that does not belong inside it.

# Adoption decision

| Finding | Decision |
|---|---|
| F-1 | `ADOPT` — as an enforced boundary, not a comment. |
| F-2 | `ADOPT` — typed `ProviderAuthenticationRequired`, checked before launch. |
| F-3 | `ADAPT` — Runlith has no editor and no terminal to present. The adaptable part is the split itself: authentication is a precondition verified outside the sandbox, and the human is told exactly what to run. |
| F-4 | `EXTERNAL_VALIDATION` — no action. |
| F-5 | `REJECT` pending primary-source verification — do not scope a subscription-credential HTTP adapter. |
| F-6 | `INSPIRED_ONLY` — relevant only if an API-key adapter is later built. |
| F-7 | `INSPIRED_ONLY` — evidence for a future ADR-0009 discussion, not a trigger for one. |

# Consequence for TASK-0102

TASK-0102 pursued the opposite direction: grant the sandboxed child access to
the platform credential store. Measured outcome on the review host — the
`mach-lookup` grant is necessary but insufficient, and closing the gap also
requires unrestricting `process-exec`, which is one of G6's core guarantees.

This study explains why that direction ran out of road. The sandboxed worker
was being asked to perform an act that every reviewed system treats as a
separate, interactive, human-present phase. The missing primitive was never
the point.

Recommended disposition:

- **Keep** the `SupportService` vocabulary. Declared-not-ambient host services,
  with the backend owning the mechanism, is sound and independently useful.
- **Drop** `SupportService::PlatformCredentials` and the Seatbelt mach-lookup
  grant. No production path should need it under the adopted pattern.
- **Open** a separate task for authentication as a verified precondition:
  `claude auth status` already returns machine-readable state, and a typed
  refusal naming the operator action replaces the current bare exit 1.

# Open questions

1. Anthropic's primary terms on subscription use by third-party orchestrators
   (F-5). Blocks any HTTP-adapter scoping.
2. Whether Runlith should speak ACP itself rather than spawning a CLI with
   flags. Not evaluated here. It would replace a private argv contract with a
   negotiated one, and it is a much larger question than the defect that
   prompted this study.
3. Whether the executor and the planner should share a provider transport at
   all. The planner is proposal-only (`--permission-mode plan`, text in, JSON
   out); the executor depends on the CLI's own tools, permission modes and
   session resume. They are not the same workload.
