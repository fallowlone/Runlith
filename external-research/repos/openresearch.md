# alphaXiv/OpenResearch

## Source revision

- repository: `alphaXiv/OpenResearch`
- repository_url: `https://github.com/alphaXiv/OpenResearch`
- default_branch: `main`
- reviewed_revision: `603558cbf18207fdf758cc50b317a06e01ba2354`
- review_date: `2026-10-10`
- version at pin: `0.2.18` (`Cargo.toml`)
- license: `MIT` (per `Cargo.toml`; `LICENSE` file not separately fetched)
- discovery_source: user-provided URL

Pin head commit: `Give paper PDF downloads a per-read timeout and a clear error (OR-468) (#622)`,
2026-10-10T07:20:27Z. Window shows an actively maintained Rust codebase with
per-issue (`OR-NNN`) commits, each carrying test plans (`cargo fmt`, `clippy -D
warnings`, `cargo test --locked`, 1081+18 tests cited).

## Executive summary

OpenResearch (`orx`) is a local-first harness and workspace for research
agents: one Rust binary (single crate `openresearch-cli`, tokio + rusqlite
bundled + axum loopback dashboard) that launches experiment runs on local /
SSH / Slurm / Kubernetes / Modal / Ray / HuggingFace / managed compute and
supervises them via a detached `orx supervise` process. State lives in SQLite
(`orx.db`, WAL) beside the work; run logs are append-only files beside the DB.

It is the closest structural cousin to Runlith reviewed so far (local-first,
Rust, SQLite, external execution, agent-harness adapters), so most findings
are `EXTERNAL_VALIDATION` or narrow `ADAPT`. The genuinely portable ideas are
the serialized backend **reattach handle**, first-class **cancel attribution**,
claim-token **lease lifecycle** for helper sessions, and fail-closed
**forwarded-run config-dir refusal**.

## Project purpose

From `README.md`: "The local-first harness & workspace for research agents."
Research agents (Claude Code, Codex, OpenCode, Cursor, Antigravity, local
models) review literature (alphaXiv/bioRxiv/PubMed), develop hypotheses, run
experiments on chosen compute, and produce artifacts. Every experiment is
recorded in a local SQL database with logs/code/artifacts on the user's
machine. Telemetry is opt-out, coarse, installation-ID-scoped, explicitly
excluding code/prompts/paths.

## Architecture map

```text
orx (single crate, src/)
├── jobs/           backend adapters: localbox, ssh, slurm, kubernetes,
│                   modal, ray, huggingface, tinker, openresearch(managed)
│                   + mod.rs: BackendDescriptor (serialized reattach handle)
├── plane/          local_plane.rs (LocalPlane::launch dispatch)
├── store.rs        SQLite truth: runs, chat_sessions/turns/spawns/wakeups,
│                   projects/experiments, leases, usage (5727 lines)
├── local/          autonomy, model, slurm helpers, shell_env, ...
├── compute.rs      validate_run_args (shared CLI+API gate), submit
├── commands/       CLI surface incl. agent spawn, supervise, up, exp
├── invocation.rs   native invocation identities per harness
└── telemetry.rs    opt-out usage events
```

Execution shape: submit natively → persist run row + `backend_json` descriptor
→ detached `orx supervise` watches beside the job → status transitions flow
through `update_status` (SQL-enforced) → wake-ups registered for agent
sessions. Logs stream by tailing pipe/redirected file with `PYTHONUNBUFFERED`
default-on.

## Findings

### F-01 — BackendDescriptor as serialized reattach handle (`src/jobs/mod.rs`)

Fixed-field struct (`kind` discriminant + optional per-backend fields,
camelCase wire) with per-kind handle accessors (`ssh_ref`, `slurm_ref`,
`k8s_ref`, `modal_ref`, `hf_ref`, `ray_ref`, `local_ref`,
`openresearch_ref`), each erroring with "was the job submitted?" when the
handle is absent. Explicit round-trip discipline: absent key does NOT survive
parse→to_json, so anything a backend must keep needs its own optional field;
tests pin older-descriptor compat and `None`-field non-serialization. Immutable
`source_digest/path/size` make a restarted supervisor independent of the
working tree. `stage_to_run_status` maps foreign stage vocabularies onto the
local 5-state vocabulary.

### F-02 — RunStatus: 5 states, SQL-enforced transitions (`src/store.rs`)

`Starting → Running → {Done, Failed, Cancelled}`; terminal states never
change; `Starting` may go anywhere (submission correction), `Running` only to
`Running/Done/Failed/Cancelled`. Enforcement is a single `UPDATE ... WHERE
(status='starting' AND ?) OR (status='running' AND ?)` — illegal transitions
apply zero rows and return false, leaving the outcome untouched. Unknown status
strings parse to `None`; `is_terminal_status` treats unrecognized strings as
non-terminal (conservative: keeps the run visible as active rather than
dropping it from in-flight views). `upsert_run` deliberately excludes
`chat_session_id` from `DO UPDATE SET` — run ownership is immutable.

### F-03 — Cancel attribution as a first-class column (OR-367)

`cancel_requested` + nullable `runs.cancel_reason`, written only by
`request_cancel`, never by submit upserts; first requester wins, rolled back
together on spawn failure. Documented invariant: nothing cancels a healthy
local run on its own — only CLI/agent-session, dashboard button, or
project-deletion. Surfaced in `orx runs` / `orx exp wait`; the in-flight launch
refusal points agents at `wait` before `cancel`, and the compute skill tells
agents to wait again after a wait timeout instead of cancelling.

### F-04 — Supervisor recovery without polling forever (OR-334, OR-383, OR-354)

`orx up` scans every 60s for submitted local runs with no live supervisor
(unchanged 5 min, so in-flight SSH launches are left alone) and starts one
instead of leaving them `starting`. A missing remote run dir becomes a
terminal error (not eternal PENDING); an SSH failure on a managed box ends the
run once the API 404s it; cancel intent still lands on `cancelled`. SSH
cancellation polls the remote process group up to 5s after TERM then escalates
to KILL (matching the container backend), with a libc-based regression test
that runs on macOS (no `setsid(1)`).

### F-05 — Claim-token lease lifecycle for helper sessions (`src/store.rs`)

`ChatSpawnState: Pending → Starting → Running → Waking → Done`; the two
`-ing` states are claims — one watcher owns the transition and holds a token
(60s TTL: `CHAT_SPAWN_CLAIM_TTL_MS`, `CHAT_TURN_LEASE_TTL_MS`), so a crash
mid-step is reclaimable rather than lost. `finished_at` absent means running
OR watcher-died-mid-turn — crash told from completion by the lease, not by a
sweeper. Wake-up registration is idempotent
(`Scheduled | AlreadyPending | AlreadyDelivered`). Related agent-turn fix
(OR-384): a spawned helper's turn waits on its background Bash commands (with
quiet-watchdog bound), otherwise the parent is woken with a progress reply as
the report.

### F-06 — Forwarded-run config-dir refusal (OR-362)

An agent session overriding `XDG_CONFIG_HOME` configures/tests compute in its
own config dir, but `exp run` forwards to `orx up`, which launched with its
own settings (e.g. different k8s namespace). The CLI now sends its config dir
with the forwarded run; `orx up` refuses with an error naming both directories
when absolute canonicalized paths differ. Fail-closed on settings mismatch
between forwarder and executor.

### F-07 — Download hygiene: per-read timeouts + mid-stream caps (OR-468)

A whole-request 30s timeout killed multi-MB PDFs mid-read with a bare decode
error. Fix mirrors their git downloader: connect/read timeouts per read (not
total), size cap enforced on the actual streamed body (not after buffering),
slow-progressing vs stalled reads covered by loopback-server tests, and the
URL named in interruption errors. Same commit thread validates `--mem` before
rendering into `#SBATCH` (newline/shell-metachar refusal at parse, OR-351
follow-up) and rejects backend-alien flags in the shared `validate_run_args`
so CLI and API cannot diverge (the check moved out of `LocalPlane::launch`
into the path both call).

### F-08 — Harness registry + per-harness permission vocab (`Cargo.toml`, `store.rs`)

`async-trait`-based `dyn Harness` registry over Claude Code / Codex / OpenCode
(/ Cursor / Antigravity at product level); capability detection per harness
(e.g. OpenCode model image-input capability carried through detection, text-only
models get a clear composer refusal instead of a success-then-error, OR-419).
`chat_sessions.permission_mode` stores provider-native modes, and schema setup
migrates retired values per harness (`codex plan → plan_mode=1 + ask`,
`claude ask/default → manual`, `opencode bypass → auto-approve`, ...), with an
explicit documented non-migration where a blanket reset would destroy a just-made
user choice (reasoning_level NOTE). `@`-file mentions read from the session
worktree via the `code-tree` endpoint (OR-211).

### F-09 — Local-first honesty posture (`README.md`, `store.rs` docs)

Opt-out coarse telemetry (no code/prompts/paths/identifiers); run logs as
plain append-only files under `run-logs/<id>.log` so tailing and appending
never contend on the DB; WAL + busy_timeout for supervise/serve concurrency;
`checkpoint(TRUNCATE)` before data-dir relocation so a file copy captures all
committed data; lifecycle file lock with legacy-path hard-link continuity;
`count_active_runs` SQL-side and unbounded so old long jobs still gate the
data-dir move.

## Bilateral comparison

Что лучше у них:

- Сериализованный reattach-дескриптор бэкенда с per-kind акцессорами и
  round-trip-дисциплиной — готовый образец для переживания рестарта демона
  исполнителями Runlith.
- Cancel attribution отдельным столбцом (first-requester-wins, upsert не
  трогает) — у Runlith отмена фиксируется беднее.
- Claim-токены с TTL вместо sweeper-ов для helper-сессий и wake-up
  идемпотентность регистрации.
- Fail-closed отказ при расхождении config-dir форвардера и исполнителя.
- Гигиена загрузок: per-read таймауты + mid-stream cap + именованная ошибка;
  валидация значений до интерполяции в backend-директивы в общем для CLI и API
  гейте.

Что лучше у меня:

- Task ≠ Execution ≠ Attempt с явной принадлежностью против их плоской строки
  run (project/experiment/run без разделения attempt).
- Журнал с хеш-цепочкой и монотонной схемой против plain-SQLite без
  аудит-упорядочивания.
- G6: exact-bound single-use approvals с fingerprint/provenance против
  provider-native permission_mode строками и миграциями значений.
- Worktree-изоляция как инвариант против запуска в чекауте сессии.
- Детерминированная верификация (G8) против agent-reported `result_markdown`.

## Verdicts

- R-01 — ADAPT, P2: reattach-дескриптор раннера (сериализованный handle +
  per-kind акцессоры + round-trip-тесты) для переживания рестарта daemon.
  Нужен ADR на формат дескриптора.
- R-02 — ADAPT, P2: cancel attribution (кто запросил отмену Attempt,
  first-requester-wins, upsert не затирает).
- R-03 — ADAPT, P3: claim-токен lease-жизненный цикл для делегированных
  helper-выполнений (reclaimable вместо sweeper).
- R-04 — ADAPT, P3: SQL-enforced таблица переходов статусов
  (zero-rows-applied = illegal, conservative unknown).
- R-05 — EXTERNAL_VALIDATION, P3: per-read таймауты + mid-stream cap как
  чеклист acquisition-путей; config-dir refusal как прецедент fail-closed при
  расхождении настроек.
- R-06 — INSPIRED_ONLY, P3: логи-как-файлы-рядом-с-БД для high-write потоков;
  idempotent wake-up регистрация.
- REJECT: single-crate форма, project/experiment-домен, `result_markdown` как
  evidence, provider-native permission строки, управляемое облако и десктоп,
  telemetry opt-out как дефолт, копирование кода адаптеров харнессов.

## Evidence

Pinned raw URLs (all at `603558cbf18207fdf758cc50b317a06e01ba2354`):

- `https://raw.githubusercontent.com/alphaXiv/OpenResearch/603558cbf18207fdf758cc50b317a06e01ba2354/src/jobs/mod.rs` (F-01; BackendDescriptor, refs, round-trip tests)
- `https://raw.githubusercontent.com/alphaXiv/OpenResearch/603558cbf18207fdf758cc50b317a06e01ba2354/src/store.rs` (F-02/F-03/F-05/F-08/F-09; 5727 lines, read head + schema + run/spawn/wakeup sections — tail sections unverified)
- `https://raw.githubusercontent.com/alphaXiv/OpenResearch/603558cbf18207fdf758cc50b317a06e01ba2354/Cargo.toml` (version 0.2.18, MIT, deps)
- `https://raw.githubusercontent.com/alphaXiv/OpenResearch/603558cbf18207fdf758cc50b317a06e01ba2354/README.md` (purpose, harnesses, telemetry statement)
- `https://github.com/alphaXiv/OpenResearch/commits/main.atom` (pin + OR-468/OR-354/OR-451/OR-334/OR-351/OR-366/OR-211/OR-384/OR-367/OR-362/OR-383/OR-419 commit evidence)
- Tree listings `src/`, `src/jobs/`, `src/plane/` at pin (layout only)

Not inspected: `plane/local_plane.rs`, `compute.rs`, `local/*`, `commands/*`,
backend bodies (`ssh.rs`, `slurm.rs`, `kubernetes.rs`, ...), `invocation.rs`,
dashboard/UI, tests, docs/.

## Research conclusion

Ближайший к Runlith из разобранных: local-first, Rust, SQLite+WAL,
внешние исполнители, адаптеры агентских харнессов. Подтверждает выбранные
направления (SQLite как truth рядом с работой, супервизия извне, fail-closed
везде) и даёт четыре переносимые механики (R-01…R-04). Широта бэкендов и
исследовательский домен — не цель; копировать нечего, кроме дисциплины.
