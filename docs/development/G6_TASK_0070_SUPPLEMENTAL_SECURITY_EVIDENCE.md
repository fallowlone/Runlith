# TASK-0070 Supplemental Security Evidence

## Verdict

Final TASK-0070 audit verdict: `G6_REMEDIATION_REQUIRED`.

Commit `2402ce2` recorded an initial provisional `G6_CLOSED` verdict. Supplemental adversarial evidence invalidated that verdict. Claude independently reproduced F-003 through the production execution path, so the provisional closure state is superseded while preserved as history.

## Repository identity

- Worktree: `/Users/artemmac/dev/runlith-task0070`
- Branch: `task-0070-independent-adversarial-g6-closure-reaudit`
- Starting audit commit: `2402ce2966b8ae6ef8300ac5e6f6173c8bedb464`
- Gate: `G6`

## Evidence artifacts

Verified artifact hashes:

- `TASK-0070-evidence-review-result.md`: `ed697ba91766e1bade4dadd4d690c0860748a9dacb2ff5116fff12ded203e619`
- `f002-full.txt`: `f2195b70c764568ee52aefccb1cb954e3ffc2090bc6222d2512492c2bc227241`
- `f003-full.txt`: `433c18e7a66734bb841b45d7e3dd951789c1c1ddff91314af29a18dadf67cf09`
- `f003e2e-full.txt`: `4b177ea469649ea157603b53df3098ca399ba9aef11264ff673e30f7e3e09051`
- supplemental probe source: `435a43ecd92803753ec3e749ff0e9479df8462d1a84c56ceb25cc9d8fb443e69`
- Claude independent security review: `9b7349e5612806bcd386ec594cac30270401eb7d04dc181801164242b95d5363`

## F-003

Status: `CONFIRMED`.
Severity: `CRITICAL`.
Closure impact: G6 blocker.

Confirmed chain:

```text
confined provider
→ authorized whole-worktree write
→ alternate Git metadata and executable helper
→ managed .git pointer rewrite
→ unsandboxed Trusted Host Git
→ provider-authored fsmonitor/helper execution
→ process runs with agentd authority
```

Production components exercised:

- `MacOsSeatbeltRunner::prepare_launch`
- `GitCli::worktree_status`

Evidence state:

- direct sandbox escape was denied;
- Trusted Host Git marker execution was reproduced;
- harmless marker was absent before the operation and present after Trusted Host Git execution.

The provider Seatbelt boundary is real. It does not secure later Trusted Host Git operations performed after sandboxed execution.

## F-002

Status: `NEEDS_MORE_EVIDENCE`.

Known evidence:

- `system.sb` is mandatory;
- no-import profile exits `134/SIGABRT`;
- `system.sb` permits read access to selected `/System` resources outside declared workspace roots;
- `/etc/hosts` and `~/.ssh` probes remained denied;
- the complete imported authority baseline is not enumerated;
- EnforcementReport does not fully describe the expanded read baseline.

## Verification evidence

- Targeted G6 suites: passed.
- Full workspace baseline: `825 passed, 0 failed, 2 ignored`.
- Existing green tests do not cover the Trusted Host Git boundary.

## Canonical state

- `completed_through`: `TASK-0070`
- `current_gate`: `G6`
- `gate_status`: `IN_PROGRESS`
- Final verdict: `G6_REMEDIATION_REQUIRED`
- Merge verdict: `DO_NOT_MERGE` for uncorrected `2402ce2`.

The Git boundary remediation remains deferred to a separate canonical task after TASK-0070 acceptance.
