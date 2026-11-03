# Proposal: G16 Desktop UI and TASK-0230 live widgets authored from a description

Status: **Ratified by the owner on 2026-11-01**, registered in
`planning/roadmap.yaml` and `planning/tasks/queue.yaml`.

## Queue-style items

```yaml
  - id: G16
    name: Desktop UI
    status: in_progress
    objective: >-
      A thin Tauri client over agentd contracts (ADR-0007, ADR-0016) with a
      customizable overview: widgets are data, never code, and any model
      output is untrusted (INV-093) until it passes a closed validator.
    scope_ref: docs/ui/proposals/TASK-PROPOSED-G16-LIVE-WIDGETS.md

  - id: TASK-0230
    gate: G16
    title: Live overview widgets authored from a plain-language description
    objective: >-
      A person describes a widget in words; agentd asks the provider for a
      versioned WidgetSpec, a closed validator admits it, the overview shows it
      with live values and keeps it across restarts.
    scope: >-
      New crates/widget-spec (types and validator, no I/O); protocol v13
      widget_core; adapters/provider-claude widget author; agentd service;
      agentctl widget; apps/desktop host and overview.
    exit_criteria_summary: >-
      Live eval on the real provider, validator rejection suite, values equal to
      gh, generation does not stall other requests, layout survives restart and
      corruption, every failure state reproduced, layout audit clean.
    depends_on: []
    blocked_by: []
    references:
      - docs/architecture/adr/ADR-0007-no-direct-frontend-shell.md
      - docs/architecture/adr/ADR-0014-local-first-privacy.md
      - docs/architecture/adr/ADR-0016-agentctl-before-complex-ui.md
```

## Decisions measured before any code

| Question | Answer | Evidence |
|---|---|---|
| Does a long generation stall the sequential channel? | Not other connections: each host command opens its own and agentd serves connections concurrently, with blocking work moved off the serve loop as `finder` does. A single request still cannot wait: the host reply budget is 5 s, and cancelling needs a handle. So `start`, `status` and `cancel` over an in-memory job with at most one running. | `session.rs:5-7`, `bins/agentd/src/lib.rs` finder dispatch, `src-tauri/src/agentd.rs` `RESPONSE_TIMEOUT` |
| Can the answer be held to a schema with tools off? | Yes. CLI 2.1.316 returns `structured_output` under `--json-schema`; `--tools ""` plus `--disallowedTools "mcp__*"` already forms the proposal-only mode. The flag must be probed like the others. | Two real runs: 5.3 s and 6.7 s, 0.012 to 0.017 USD equivalent; the weather request was refused |
| Who picks the model? | The provider default, as the planner does (`claude-code/provider-native-default`), measured as `claude-opus-5-5`. G14 routes nothing live yet (ADR-0049); it takes over when it does. | `planner.rs`, ADR-0049 Consequences |
| Is a provider call allowed outside the existing boundary? | No. It goes through the same Policy and Runner path as `SecuredClaudePlanner`, with an empty isolation directory as cwd. | `bins/agentd/src/planning.rs` |

## Facts that shape the GitHub source

- `open_issues_count` cannot count pull requests: `cli/cli` reports 1101 while
  `gh pr list` reports 53. `GET /pulls?state=open&per_page=1` with `Link`
  `rel="last"` returned 53, and no `Link` header means 0 or 1 by body length.
- Unauthenticated limit is 60 requests per hour per IP. Per the GitHub docs a
  304 is free only for authenticated requests, so every anonymous poll counts:
  two widgets every 5 minutes use 24 per hour.
- `reqwest` 0.13 is already in the desktop lockfile, without TLS. Enabling it
  adds a TLS backend, the only new dependency this proposal expects.

## Non-goals

Drag to reorder, sources beyond `github.repo.stars` and `github.repo.open_prs`,
arbitrary URLs, automations that act rather than refresh a value, GitHub tokens.
