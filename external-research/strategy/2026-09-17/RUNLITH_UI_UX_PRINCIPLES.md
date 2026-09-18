# Runlith: UI and UX principles

Research date: 17 September 2026. Status: proposed product principles, not a visual implementation specification. Findings come from official design explanations and interaction documentation, not instrumented hands-on comparisons. No existing Runlith desktop implementation was found in the inspected main checkout. [Evidence register](EVIDENCE_REGISTER.md).

**Desired feeling:** calm control over consequential work. A user should be able to leave, return, understand what happened, and make the next decision without reading a conversation transcript.

The interface should be precise enough for an experienced engineer and simple enough that the engineer's manager can understand a delivery state. It should not require either person to understand the scheduler's internal state machine.

During this study, main gained a separate [desktop UI/UX foundation](../../../../runlith/docs/ui/research/2026-09-17-runlith-ui-ux-research.md), based on the same implementation snapshot. It supplies a more detailed Task-first object map, panel alternatives, query gaps and proposed design research. The principles here complement it with the customer and commercial frame. Neither document is an accepted UI contract. The existing Task should remain the initial persistent work object; “engineering change” is an outcome-oriented presentation hypothesis, not authorization to create a competing lifecycle.

## 1. What professional desktop products teach

### Cursor: keep the work artifact close

**Information architecture:** code, conversations, changes and remote work remain connected to a project.

**Interaction:** the developer can inspect and adjust the produced artifact; cloud-agent material includes browser and test artifacts.

**Visual/hierarchy lesson:** code and the resulting change deserve more emphasis than the assistant's persona. Screenshots and recordings are inspectable evidence of particular behavior, not a general certificate of correctness.

**Runlith application:** every delegated change should lead directly to its diff, evidence and unresolved obligations. A transcript is secondary supporting material.

**Limit:** an editor can assume code is the main activity. Runlith's separate surface must earn its existence through acceptance and coordination.

Sources: [cloud-agent workflow](https://cursor.com/docs/cloud-agent), [artifact-oriented computer use](https://cursor.com/blog/agent-computer-use).

### Warp: group action, output and context

**Information architecture:** terminal Blocks associate a command with its output instead of treating the terminal only as an undifferentiated text stream.

**Interaction:** contextual actions operate on a meaningful object.

**Visual lesson:** boundaries are useful when they explain which result belongs to which action. Repeated borders around everything dilute that benefit.

**Runlith application:** show an attempted check, its subject, result and logs together. Link an error to the decision it blocks.

**Limit:** raw command output is useful for diagnosis; it is not the overview most users need.

Source: [Warp Blocks](https://docs.warp.dev/terminal/blocks).

### Linear: hierarchy before decoration

**Information architecture:** a small set of work objects and consistent navigation handles substantial team complexity.

**Interaction:** stable patterns and predictable headers reduce the need to relearn each view.

**Visual lesson:** Linear's design explanation emphasizes clearer hierarchy and reducing distracting chrome. Density can remain high when alignment and emphasis are disciplined.

**Runlith application:** stable list/detail behavior, restrained status colors and consistent locations for primary actions.

**Limit:** copying Linear's visual style would not give Runlith its product clarity. Linear also now participates in execution, making it a competitive reference.

Sources: [recent design reasoning](https://linear.app/now/behind-the-latest-design-refresh), [earlier redesign](https://linear.app/now/how-we-redesigned-the-linear-ui).

### Raycast: contextual action and keyboard fluency

**Information architecture:** search identifies an object or command; the Action Panel exposes operations relevant to the selected result.

**Interaction:** Enter performs a primary action, keyboard shortcuts are visible, and secondary actions remain discoverable.

**Visual lesson:** a small, stable surface can expose considerable power if selection and action are unambiguous.

**Runlith application:** one palette for “open change,” “review pending decision,” “show evidence,” and scoped workflow actions. Selection context must remain visible.

**Limit:** permissions should never depend on a hidden shortcut or an ambiguous currently selected workspace.

Source: [Action Panel](https://manual.raycast.com/action-panel).

### Arc: distinguish workspace organization from identity

**Information architecture:** Spaces organize browsing contexts, while Profiles separate identities and browser data.

**Interaction:** switching a work context need not mean switching every underlying account.

**Visual lesson:** a recognizable context reduces accidental work in the wrong place.

**Runlith application:** distinguish project, repository/workspace, execution location and provider account. They are related but not interchangeable.

**Limit:** do not proliferate nested Spaces, Missions and Environments as three competing ways to find the same change. Arc's current maintenance-oriented positioning is a reminder that distinctive UX does not independently prove business durability.

Sources: [Spaces](https://resources.arc.net/hc/en-us/articles/19228064149143-Spaces-Distinct-Browsing-Areas), [Profiles](https://resources.arc.net/hc/en-us/articles/19227964556183-Profiles-Separate-Work-Personal-Browsing), [current status](https://arc.net/).

### Figma: progressive disclosure must preserve frequent work

**Information architecture:** a central artifact is supported by context-sensitive controls.

**Interaction and visual lesson:** Figma's UI3 account describes revisions after feedback: floating panels gave way to fixed, resizable panels, and some hidden properties became easier to access. Visual openness did not compensate for slower inspection or poor behavior on smaller screens.

**Runlith application:** evidence and a diff should remain visible while a user evaluates a decision. Do not hide important acceptance conditions behind decorative icons.

**Limit:** a control-plane product does not need an infinite canvas merely because its task graph is a graph.

Source: [Figma's UI3 design account](https://www.figma.com/blog/our-approach-to-designing-ui3/).

### VS Code: stable regions with optional depth

**Information architecture:** navigation, working artifact, auxiliary panels and status have consistent roles; Profiles separate configurations for different uses.

**Interaction:** command discovery and keyboard access coexist with visible navigation.

**Visual lesson:** users tolerate high information density when panels are predictable and can be reduced for focus.

**Runlith application:** a stable main work area and an optional evidence/diagnostic inspector. Remember useful layout preferences.

**Limit:** adopting every panel and extension surface would recreate an IDE without its existing ecosystem.

Sources: [user interface](https://code.visualstudio.com/docs/editing/getting-started/userinterface), [Profiles](https://code.visualstudio.com/docs/configure/profiles).

### JetBrains products: expertise without mandatory density

**Information architecture:** deep capabilities are organized into contextual tool windows and a central work area.

**Interaction:** Search Everywhere offers a path to features and objects without navigating every menu.

**Visual lesson:** compact presentation is valuable for experts, but should be a deliberate density choice rather than unreadably small defaults.

**Runlith application:** make advanced evidence and policy inspection accessible without giving them equal prominence in every session.

**Limit:** Runlith does not initially have the frequency or breadth of an IDE to justify an IDE-sized navigation structure.

Sources: [New UI](https://www.jetbrains.com/help/idea/new-ui.html), [Search Everywhere](https://www.jetbrains.com/help/idea/searching-everywhere.html?keymap=primary_default_for_windows).

### Things 3: separate intention from urgency

**Information architecture:** Today, Upcoming, Anytime and Someday distinguish when someone intends to act; a deadline is a different concept.

**Interaction:** users can organize attention without pretending every task is urgent.

**Visual lesson:** restrained hierarchy lets an actionable list feel manageable.

**Runlith application:** “needs your decision,” “ready to review” and “running without intervention” should be different views. A blocked low-priority change need not generate an emergency.

**Limit:** autonomous processes also need operational state, ownership and budget; a personal to-do metaphor cannot carry those alone.

Sources: [Things scheduling guidance](https://culturedcode.com/things/support/articles/4001304/), [Things support reference](https://culturedcode.com/things/support/articles/2803579/).

### Superhuman: optimize repeated decisions

**Information architecture:** Split Inbox creates focused queues instead of requiring one universal chronological stream.

**Interaction:** keyboard fluency and a discoverable command palette support repeated decisions.

**Visual lesson:** clear selection and a predictable next action matter more than decorative dashboard metrics.

**Runlith application:** batch similar review decisions and make the next unresolved item obvious.

**Limit:** “inbox zero” must not encourage approving consequential changes merely to clear a counter. Dismissal, delegation and risk acceptance are different actions.

Sources: [command-palette design](https://blog.superhuman.com/how-to-build-a-remarkable-command-palette/), [default Split Inbox](https://help.superhuman.com/hc/en-us/articles/46005619081101-Default-Split-Inbox), [custom Split Inbox](https://help.superhuman.com/hc/en-us/articles/46005636204941-Custom-Split-Inbox).

## 2. The product object should be a change

The initial user hierarchy should be:

**Workspace → project/repository → engineering change → evidence and decisions.**

An engineering change is a named intended outcome, such as “Fix duplicate invoice retries,” with a scope, acceptance criteria and delivery destination. It can contain tasks and multiple attempts. The existing Task model remains authoritative for executable work; the interface must not create a second conflicting lifecycle.

Agent, provider, model, attempt and sandbox are attributes or inspection views. They should not be the top-level organization merely because the implementation has those modules.

Recommended primary destinations:

| Destination | User question | Default content |
|---|---|---|
| Attention | What needs my judgment now? | Blocking decisions, work ready for review, significant failures |
| Work | What have I delegated and what is happening? | Change list with outcome, owner, state, next action and budget signal |
| Project | What does this repository need us to know? | Relevant instructions, accepted decisions, recurring procedures and integration links |

Settings, provider setup, policies and diagnostics remain discoverable secondary destinations. Do not launch with Agents, Memory, Policies, Evaluators, Runners, Budgets and Graph as equal sidebar categories.

The default overview should answer four questions in seconds: **What changed? What is waiting? What is safe to leave running? What should I decide next?**

## 3. Separate work state from attention state

Internal state is necessary for correctness; product state is necessary for comprehension. Keep an explicit mapping and do not collapse missing evidence into success.

| User-facing state, proposed | What it means | What must be visible |
|---|---|---|
| Queued | Work is accepted but has not started | What it is waiting for and whether a start time is known |
| Working | An owner is actively pursuing the change | Last meaningful activity, scope and spend/budget |
| Needs a decision | Progress requires a human choice | Exact question, relevant diff/evidence, consequences and alternatives |
| Paused or interrupted | Work is not currently progressing | Whether the worker stopped, what survived, and available recovery |
| Ready for review | A reviewable candidate exists | Required checks, missing evidence, changed files and unresolved risks |
| Accepted | The stated acceptance authority approved a particular revision | Who/what accepted it and the acceptance revision |
| Delivered | A specified external delivery event was observed | PR/merge/deployment reference and its actual status |
| Abandoned | The owner ended the intention to pursue this change | Preserved partial work and reason, once supported by domain semantics |

“Working” must not be inferred from a spinner alone. A stale heartbeat, a provider quota wait, an unavailable verifier and a slow test are different explanations.

These labels are a design proposal. Current code does not support every lifecycle operation, including a complete Task cancellation contract. The UI must follow supported semantics rather than fake them.

## 4. The primary journey

1. **Delegate:** choose an existing issue or write an outcome. Reuse repository context and existing tools. Do not require the user to design an agent organization.
2. **Check the agreement:** show outcome, non-goals, allowed scope, proposed acceptance and budget. Small work should have a short path; consequential uncertainty should be explicit.
3. **Leave:** make execution location and availability clear. A sleeping local laptop is different from a remote worker.
4. **Return:** show a concise account of changes, checks, interruptions and decisions since the last visit.
5. **Review:** present the exact candidate diff alongside requirement coverage, failed/skipped checks and relevant observations.
6. **Decide:** accept, request a bounded revision, take over, or abandon when supported. Explain which action changes authority or spends more.
7. **Deliver:** show the actual PR/merge/deployment state. Preserve links between delivery and the accepted revision.

There should be no need to read all planner deliberation to decide whether to accept a change. Provide an explanation with evidence references, not an unfiltered reasoning stream.

## 5. An evidence-first review surface

The review surface should give most space to the artifact and its acceptance. A useful order is:

- intended outcome and candidate revision;
- concise change explanation linked to the relevant diff;
- acceptance criteria with observed check results;
- remaining uncertainty, failures and skipped checks;
- proposed decision and its effect;
- optional timeline, logs and provider transcript.

A check row should say what ran, where, against what, when, and with what result. “Tests passed” without the subject revision or omitted checks is inadequate.

If code changes after verification, mark the affected evidence stale. Do not preserve an undifferentiated green badge. If a model reviewer supplies an opinion, label it as such.

Avoid a universal “93/100 reliability” score. It blends incommensurable evidence and implies calibration that has not been established. Prefer concrete claims such as “8 required checks passed; manual acceptance pending; performance check did not run.”

## 6. Keyboard, contextual actions and approvals

Use one discoverable command system. Commands should resolve against the visible project and selected change; show that context before a consequential action.

Maintain a visible primary action. Offer keyboard shortcuts for frequent navigation and review actions, with pointer equivalents and accessible focus behavior. Avoid shortcuts that silently approve multiple changes.

Use progressive disclosure according to decision relevance:

| Always available at the decision | Available on demand |
|---|---|
| What is being authorized | Raw IPC payload or internal identifiers |
| What changed since the previous proposal | Full chronological event log |
| Scope, recipient and cost implication | Detailed adapter diagnostics |
| Required evidence and missing checks | Complete provider transcript |
| Whether the action is reversible | Implementation topology |

“Approve” is insufficient when it could mean approve a plan, grant filesystem access, accept a result or publish externally. Use the actual verb and state its effect. Re-prompt only when authority or the relevant proposal changes, not at every harmless step.

HumanDecisionRequest and security Approval can share an Attention queue while retaining different types, validity rules and actions. A recorded answer is not necessarily consumed, and a consumed authorization is not proof that the operation occurred. Hard policy denial must not become an “approve anyway” button. The adjacent UI research identifies these distinctions explicitly; preserve them in any simplified presentation.

## 7. Visual principles

**Density:** optimize for comparison of meaningful work. Use aligned rows and concise metadata. Offer a comfortable default and an optional compact mode after usability evidence; density is not the same as small text.

**Typography:** use a restrained hierarchy with readable body text and tabular numerals where counts or budgets need comparison. Reserve monospace for code, commands and technical identifiers. Do not make every label look like a terminal.

**Spacing:** express relationships with consistent grouping. Separate decisions more strongly than repeated metadata. Avoid cards around every field.

**Color:** use a neutral work surface and limited semantic accents. Color must not be the only status signal. A red failure should explain whether the work stopped and what the user can do.

**Motion:** use motion for continuity and state changes, not to simulate productivity. Respect reduced-motion preferences. Avoid streaming activity that constantly displaces the item being reviewed.

**Accessibility:** keyboard completion, visible focus, readable contrast, semantic labels and screen-reader announcements belong in initial quality criteria. Do not claim accessibility conformance from a visual review.

**Trust:** timestamps, artifact identities, explicit unknowns and effective controls are stronger trust signals than shields, glowing avatars or animated agent activity.

These are proposed principles, not copied design tokens or an unsupported claim that one font/spacing scale is universally optimal.

## 8. Notification policy and human attention

Notify when a human can change the outcome: a required decision, a useful review candidate, a material budget deviation or a failure that cannot recover within delegated authority.

Batch routine completion and low-priority updates. Preserve a “since you last checked” digest. Let users distinguish urgent delivery constraints from optional maintenance.

Every notification should name the change, explain why attention is needed and open the relevant evidence. An alert that merely says “agent failed” transfers diagnosis to the user.

Do not reward the product team for increasing session time, notification clicks or number of approvals. Measure time to an informed decision and successful completion with appropriate attention.

## 9. Local-first and cost communication

Show the execution location and relevant data recipients before the first delegation and whenever they materially change. “Local project” can still use a remote model. Avoid a single privacy badge that hides this distinction.

Expose a whole-workflow budget: planning, execution, retries, verification and environment charges where measurable. Separate known spend, reserved exposure and unavailable estimates. Do not present missing usage as zero.

If the app loses contact with a worker, say what is known and what is unresolved. Do not offer “retry safely” before the system has established that an external side effect did not already occur.

## 10. What to remove from the initial interface

- A DAG as the default home screen. Keep it as an optional dependency diagnostic.
- Agent employee cards, personality selectors and council configuration.
- Separate top-level navigation for every architecture subsystem.
- A generic node-based automation builder.
- Reliability scores without calibration.
- A global live log replacing an actionable status summary.
- Repeated confirmation for operations already authorized within the same scope.
- A second issue tracker, code editor or company wiki.

These are scope recommendations. They do not remove underlying policy or evidence requirements.

## 11. Research and acceptance criteria for the UX

Before polishing a complete desktop shell, test a small interactive prototype with the proposed first customers. Use actual or realistically anonymized maintenance changes, including a failed check and an interrupted worker.

Proposed tasks:

| Task | What to observe | Initial decision threshold, a hypothesis |
|---|---|---|
| Identify what needs attention | Time and correctness without guidance | Most participants choose the correct next item within 10 seconds |
| Explain whether a change is accepted or delivered | Misinterpretations of status | No participant confuses “agent finished” with verified acceptance in the test |
| Review stale or incomplete evidence | Whether missing checks are noticed | Every participant notices the deliberately missing required evidence |
| Recover interrupted work | Ability to explain what survives and what retry does | No participant assumes ambiguous side effects are automatically safe |
| Delegate a small change | Time, setup errors and perceived ceremony | Faster than reconstructing the same agreement across existing tools |
| Use keyboard and assistive interaction | Reachability and focus order | Core path works without pointer-only actions |

These are small-study gates, not statistically established safety guarantees. Record mistakes, time and explanation quality rather than only asking whether the interface looks professional.

The product should feel trustworthy because the user can understand and control the work, even when the agent is wrong.
