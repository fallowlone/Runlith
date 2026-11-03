# One alternative launch: system-prompt replacement

Recorded before capture on 2026-09-30. This is the only alternative evaluated
in this bounded pass. It is diagnostic feasibility evidence for owner B.

Configuration: `ClaudeConfig::default().proposal_only().with_system_prompt("role")`,
CLI 2.1.283, executable SHA-256
`d8cb1e5c79684cc12a8bfc813e3a2073406921b6245744b3009be3ab5651d21e`.
Exact argv, followed by the unchanged benign marker prompt:

```text
-p --safe-mode --tools "" --disallowedTools "mcp__*" --output-format json --system-prompt role
```

Existing contract at starting HEAD 84fee71111e64acb7b9a77bdaefe154e0bf53378:
`config.rs::with_system_prompt` accepts bounded adapter-authored replacement
text; `argv.rs::build_argv` emits this flag after JSON output; its existing
`variadic_tool_flags_never_swallow_the_prompt` test uses exactly `"role"`.
`capabilities.rs::probe_capabilities` recognizes `--system-prompt`.
`planner.rs::bound_planner_config` already combines proposal-only with system
replacement in the production planner; the present marker is not a planner
functional test. These provider sources are byte-identical at compatibility
main 5362f6efb7003b8c0dd24ede9bd5eb904791f525. This grants no production
credential delivery or local network scope: those existing gates still refuse.

Hypothesis: replacing the built-in prompt might eliminate the dynamic
system-role/environment message, its per-message effort, and associated
cache/legacy beta requirements. A complete emitted sequence will confirm or
refute that reduction. Retained exact beta values still require their own
semantic evidence; removing a prompt does not prove their behavior. No header,
field, or beta is stripped, and no new CLI/environment switch is introduced.

Capture may run only after write checks, harmless Seatbelt launch and mechanical
filesystem/network/process preflight pass. A failed isolation preflight ends
capture work without unsandboxed fallback. Fresh private environment, dummy
in-memory API-key-class material, local synthetic endpoint and the original
finite capture limits apply. Historical captures and source hashes are kept.
