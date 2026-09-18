# Simple Attention Sparsification (SAS)

## Source revision

- repository: `Tencent-Hunyuan/Simple-Attention-Sparsification`
- repository_url: `https://github.com/Tencent-Hunyuan/Simple-Attention-Sparsification`
- default_branch: `release`
- reviewed_revision: `release@c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`
- commit_sha: `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`
- commit_date: `2026-09-14 01:44:26 -0700`
- commit_subject: `Enhance README with badges for resources`
- tag_version: none found for the reviewed revision
- review_date: `2026-09-15`
- linked_paper: `arXiv:2609.13141`, *SAS: Simple Attention Sparsification via End-to-End Optimization of Context Ranking*
- inference_submodule: `rayleizhu/sglang@2a090d525cc52dc8eb70059543271baef34c5696`
- benchmark_submodules pinned by gitlink: `ShishirPatil/gorilla@6ea57973c7a6097fd7c5915698c54c17c5b1b6c8`, `meituan-longcat/vitabench@742e240855bf8686a0842360749d5ea970ea3987`

The review used a fresh clone of the exact `release` revision above. The SGLang submodule was initialized at its exact gitlink revision because the production inference path for SAS lives there. Gorilla/BFCL and VitaBench were inspected through the parent repository's pinned gitlinks, patches, and launch scripts; their full source trees were not required to establish the SAS-specific architecture and trust boundaries described below.

No production Runlith code, Cargo manifests, canonical roadmap, or canonical task queue was changed by this research.

## Executive summary

SAS is a model-runtime optimization project rather than an agent/orchestration system. Its main engineering contribution is a learned block selector that keeps hard Top-K sparse computation while giving the language-modeling loss a differentiable path into the selector through continuous log-space gates. The base Qwen3 model is frozen; only a small per-layer router is trained and exported. At inference, the SGLang fork keeps prefill dense and sparsifies decode by maintaining per-block compressed-key summaries plus a small rolling accumulator for the current incomplete block.

The most useful Runlith lesson is **objective alignment**. SAS shows that optimizing a selector against a proxy target (dense-attention imitation) can be weaker than optimizing the selector against the downstream objective that ultimately matters. Runlith has already reached the analogous architecture decision at the orchestration level: route/context/efficiency choices should be judged by verified outcomes and `VerifiedDeliveryCost`, not token count, model popularity, or another surrogate metric. This is `EXTERNAL_VALIDATION`, not a new architecture gap.

The second strong lesson is **fail-closed compatibility for optimization-specific state**. SAS inference is not merely "KV cache + different kernel". It introduces summary slots and a rolling accumulator whose lifecycle must remain synchronized with request/KV ownership. The SGLang backend therefore rejects modes whose lifecycle cannot preserve that state, including disaggregated serving, mixed prefill/decode, streaming sessions, and speculative decoding. This closely validates Runlith's hard eligibility gates and its rule that optimized/provider-specific runtime state must remain derived, capability-gated, and non-authoritative.

The third lesson is negative and important: sparsifying the expensive operation does not make the selector free. The paper reports that at very long context the global selector and Top-K ranking become the dominant decode cost; at 512K, Top-K reaches 90% of the sparse decode step. The implementation likewise materializes per-query block score state during training. This directly validates Runlith's `EffectiveContextCost`/`Attention Load` model: selection/transformation/routing overhead must be included in the optimization bill.

No material `NOVEL_GAP` was found for Runlith. The potentially transferable ideas are already designed, already implemented at a stronger authority boundary, or belong below Runlith in the model-runtime backend. Consequently this review creates **no candidate ADR and no candidate implementation task**.

Security review found one clear trust-boundary weakness in the evaluation launcher: SGLang is always started with `--trust-remote-code`, so choosing an untrusted model repository can also grant Python-code execution to that serving process. Runlith should continue to place local model loading behind Policy, artifact trust/provenance, and Runner isolation. The VitaBench script handles its cloud API key more carefully by writing it to a mode-0600 temporary file and registering cleanup, but a hard process/host failure can still leave that file behind. SAS's gate weight load uses `torch.load` without an explicit `weights_only=` argument; in the reviewed supported environment `torch>=2.8` makes the default safe (`weights_only=True`), so unsafe gate deserialization is **not supported as a finding for the intended environment**.

## Project purpose

SAS is post-training attention sparsification for pretrained Transformers. For each query, a lightweight selector scores historical KV blocks. The runtime attends only to a bounded set of selected blocks, while training inserts continuous selector gates into attention logits so the normal language-modeling loss can train context ranking end to end.

The repository provides:

- router-only sparse-attention training for Qwen3 4B/8B/14B;
- custom Triton kernels for block scoring and sparse attention with backward support;
- distributed training/checkpointing through VeOmni/FSDP;
- export into a small `AttnGates` artifact;
- an SGLang fork implementing decode-time sparse attention over paged KV;
- reasoning, LongBench-E, BFCL, and VitaBench evaluation harnesses.

It does **not** provide a durable agent workflow engine, Task/Execution/Attempt lifecycle, policy engine, application database, human approval system, or completion authority. BFCL and VitaBench are evaluation workloads driven through external benchmark CLIs, not SAS-owned agent orchestration.

## Architecture map

```text
Repository
├── Interfaces
│   ├── README + shell training/eval entrypoints
│   ├── config files under configs/
│   └── OpenAI-compatible SGLang HTTP eval endpoint
├── Orchestration
│   ├── VeOmni distributed trainer
│   └── shell evaluation drivers
├── Domain
│   ├── AttnGateRouter
│   ├── SimpleSparseAttention
│   ├── block-scoring kernel
│   └── sparse attention kernel
├── State/Persistence
│   ├── distributed training checkpoints
│   ├── router-only AttnGates artifact
│   ├── SGLang paged KV cache
│   ├── per-block compressed-K summary cache
│   └── per-request rolling max|min|sum accumulator
├── Providers
│   ├── Hugging Face/Qwen3 model loading
│   ├── SGLang/FlashInfer inference
│   └── DeepSeek cloud evaluator/user simulator for VitaBench
├── Tools/Capabilities
│   └── no product capability system; benchmark CLIs only
├── Execution
│   ├── FSDP router-only training
│   ├── Triton forward/backward kernels
│   └── dense prefill + block-sparse decode
├── Recovery
│   ├── DCP training checkpoint/resume
│   ├── server readiness/cleanup logic
│   └── bounded HTTP eval retries
├── Security
│   ├── model/repository trust inherited by serving process
│   ├── temporary benchmark secret material
│   └── no application authorization layer
└── Observability
    ├── training loss/grad/LR/environment metrics + optional W&B
    ├── profiler hooks
    ├── timestamped eval directories/server logs
    └── benchmark scores; no unified immutable run manifest
```

### Repository-level architecture answers

| Question | Reviewed conclusion |
|---|---|
| Where is business logic? | The model-level domain is in `src/sas/core/attn/` and `src/sas/core/kernels/`; training orchestration is in `src/sas/train/trainer.py`; serving semantics are in the pinned SGLang submodule. |
| Who owns lifecycle? | Training lifecycle is owned by the VeOmni-based trainer; inference request/KV lifecycle is owned by SGLang. There is no application Task lifecycle. |
| Where is canonical state? | During training, model/router + optimizer + scheduler/dataloader/RNG checkpoint state. During serving, SGLang owns request/KV state; SAS adds derived summary/rolling state. |
| How are jobs/tasks represented? | They are scripts/training steps/benchmark requests, not durable domain Tasks. |
| Explicit state machine? | No product workflow state machine. SGLang request/cache lifecycle is implicit in backend/pool types and forward modes. |
| Scheduler? | VeOmni/FSDP and SGLang schedule compute. No Runlith-like durable scheduler exists. |
| Recovery owner? | Trainer/DCP owns training resume; eval scripts own process restart/cleanup; HTTP client owns small bounded request retries. |
| Crash/restart? | Training has meaningful checkpoint resume. Eval/server state is process-local and generally rerun. |
| Concurrency? | Distributed FSDP training; SGLang DP/TP; bounded async HTTP semaphore in eval client. |
| Atomic boundaries? | Checkpoint save/load and model/export files; no transactional application persistence. |
| Duplicate mutable truth? | Main inference hazard is auxiliary summary/rolling state drifting from KV/request ownership; the backend explicitly guards unsupported lifecycle modes. |
| Versioned schemas/contracts? | Python package/config fields and checkpoint config; compatibility is largely structural rather than a formal schema version protocol. |
| Provider abstraction? | Model/framework specific: current training/inference path is strongly Qwen3/HF/SGLang specific. |
| Can provider mutate business state? | No product business state exists. |
| Policy/authorization? | Absent. Local scripts assume operator authority. |
| Capabilities? | Runtime feature support is checked by backend configuration/guards, not by an authorization capability algebra. |
| Execution receipts/evidence? | Training checkpoints/logs and benchmark result files are evidence, but there is no immutable execution receipt model. |
| Logs/audit/observability? | Operational logs/metrics exist; security audit/event provenance does not. |
| UI/API vs domain? | No UI. HTTP evaluation interface is outside the training/domain implementation. |

## Canonical state ownership

SAS has two relevant state owners.

**Training:** the trainer owns the current router parameters and optimizer/runtime state. The backbone is deliberately frozen. Checkpoints persist enough state to resume step ordering and stochastic/data-loader progression: optimizer, global step, LR scheduler, dataloader, environment meter, and Torch RNG state. The resume path also repairs optimizer param-group hyperparameters that DCP may silently omit.

**Inference:** SGLang remains the primary request/KV owner. SAS adds two derived state structures alongside the KV cache:

1. one compressed-K summary per logical KV block; and
2. a per-request rolling `[max|min|sum]` accumulator for the incomplete block.

These derived structures are not independently authoritative. They are allocated/freed at the same lifecycle points as KV state and are invalid if transferred/reused without matching semantics. The backend's fail-closed guards make that ownership dependency explicit.

For Runlith, the analogous rule is already established: provider/session/runtime caches may accelerate execution, but product-owned Task/Execution/Attempt, Policy, Context Continuity, Verification, and durable evidence remain authoritative.

## Strong engineering decisions

### SAS-F01 — Train the selector against the downstream objective

**finding_id:** `SAS-F01`

**source:** implementation + paper

**repository:** `Tencent-Hunyuan/Simple-Attention-Sparsification`

**revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

**file / symbol / region:**

- `src/sas/core/attn/simple_sparse_attention.py` — `SimpleSparseAttention`, `forward`, approximately `295-340`, `403-530`.
- `src/sas/train/trainer.py` — `freeze_all_but_router`, `57-61`; training loss/backward, `441-447`.
- `README.md`, `9-26`.
- supplemental: `arXiv:2609.13141v1`, method and ablation sections.

**observation:** hard Top-K still bounds sparse computation, but continuous selector gates are injected into the selected attention logits. The LM loss therefore backpropagates into the selector while the backbone remains frozen. The selector is optimized for downstream prediction behavior rather than solely imitating dense-attention rankings.

**confidence:** `HIGH`

**Problem:** a proxy objective can optimize the selector for resemblance to a baseline rather than for the final behavior under a fixed resource budget.

**Mechanism:** retain the hard selection boundary for execution cost, add a differentiable continuous path for relative ranking, and train using the final task objective.

**Why it works:** optimization pressure is attached to the consequence that actually matters, including cross-layer effects that a layer-local imitation target may miss.

**Limitations:** this is model training, not Runlith orchestration. It does not imply that every Runlith decision should become gradient-trained.

**Failure modes:** poor gate formulation can collapse ranking quality even while training loss looks good; the paper's ablations show lower training loss alone is not sufficient evidence of a better selector.

**Complexity:** custom forward/backward kernels and post-training are substantial model-runtime work.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith already defines `VerifiedDeliveryCost`, quality/safety floors, local verified outcomes, routing feedback, and benchmark evidence as inputs rather than universal truth.

**Decision:** `EXTERNAL_VALIDATION`.

### SAS-F02 — Freeze the expensive base and persist only the learned delta

**finding_id:** `SAS-F02`

**source:** implementation

**repository:** same

**revision:** same

**file / symbol / region:**

- `src/sas/train/trainer.py` — `freeze_all_but_router`, `57-61`; optimizer selects router parameters, approximately `222-247`; final gate-only export, `534-550`.
- `src/sas/train/export_gate.py` — `_collect_gate_tensors`, `74-89`; `export_attn_gates`, `136-201`.

**observation:** all base model parameters are frozen. The optimizer contains router parameters only. Final deployment exports a small gate artifact plus configuration/tokenizer metadata instead of duplicating the full base model.

**confidence:** `HIGH`

**Problem:** copying or mutating a large shared base model increases storage, deployment, identity, and rollback complexity when only a small adaptation changed.

**Mechanism:** explicit immutable base reference + independently versioned learned delta.

**Why it works:** the deployment artifact corresponds directly to the changed state and can be replaced independently from the base weights.

**Limitations:** the artifact still depends on exact base architecture/configuration and the SGLang adapter contract. `base_model` identity is not content-addressed in this repository.

**Runlith overlap:** the exact ML technique is below Runlith. As a systems principle, Runlith already prefers small replaceable adapters/derived artifacts over duplicate canonical truth.

**Decision:** `INSPIRED_ONLY`.

### SAS-F03 — Fail closed when optimization-specific state cannot remain coherent

**finding_id:** `SAS-F03`

**source:** implementation in parent repository and pinned SGLang submodule

**repository:** parent `Tencent-Hunyuan/Simple-Attention-Sparsification@c7d7101...`; inference submodule `rayleizhu/sglang@2a090d5...`

**file / symbol / region:**

- `src/sas/train/export_gate.py` — `derive_seerattn_fields`, `41-71`.
- `third_party/sglang-blocksparse/python/sglang/srt/configs/model_config.py` — `_resolve_and_verify_seer_attn_config`, `328-404`.
- `third_party/sglang-blocksparse/python/sglang/srt/layers/attention/blocksparse/seer_attn/backend.py` — `_init_sparse_config`, `84-180`.

**observation:** export and serving reject unsupported router configurations. The runtime also rejects incompatible page layout, missing summary tables, disaggregated serving, mixed prefill/decode, streaming sessions, and speculative decoding because those modes would make summary/rolling state absent, stale, leaked, or incorrectly advanced.

**confidence:** `HIGH`

**Problem:** an optimization can appear to "run" while silently producing wrong outputs if its auxiliary state assumptions are violated.

**Mechanism:** encode assumptions as startup/export eligibility checks and stop before execution when the runtime cannot maintain the necessary state.

**Why it works:** unsupported composition becomes an explicit capability failure instead of latent accuracy corruption.

**Limitations:** hard guards reduce feature compatibility until transfer/rebuild semantics are implemented.

**Runlith overlap:** `ALREADY_DESIGNED` and partly `ALREADY_IMPLEMENTED` at a stronger layer. `INV-060` requires hard capability/security/privacy/resource eligibility gates before scoring; G6 Policy/Runner paths fail closed for unsupported authority/isolation requirements.

**Decision:** `EXTERNAL_VALIDATION`.

### SAS-F04 — Auxiliary summary state follows the owner resource lifecycle

**finding_id:** `SAS-F04`

**source:** implementation in pinned SGLang submodule

**repository:** `rayleizhu/sglang`

**revision:** `2a090d525cc52dc8eb70059543271baef34c5696`

**file / symbol / region:**

- `python/sglang/srt/mem_cache/blocksparse_summary.py` — module contract `3-14`; `_maybe_alloc_summary_slots`, `35-122`; `_maybe_free_summary_for_release`, `175-200`.
- `python/sglang/srt/mem_cache/seer_attn_memory_pool.py` — `SeerAttnTokenToKVPool`, `56-129`.
- `python/sglang/srt/layers/attention/blocksparse/seer_attn/backend.py`, `120-180`.

**observation:** block summary slots are allocated/freed at KV lifecycle points. Exhausting the summary pool raises a hard error because returning with a sentinel would silently make blocks appear empty. The rolling accumulator uses reduction state rather than buffering a whole raw block and resets through block-position semantics.

**confidence:** `HIGH`

**Problem:** independently managed derived caches easily outlive, lag, or diverge from the canonical resource whose content they summarize.

**Mechanism:** co-locate allocation/free hooks with owner-resource lifecycle; treat impossible derived-state accounting as corruption; use small incremental state where full buffering is unnecessary.

**Why it works:** there is one lifecycle clock for the source and its derived index.

**Limitations:** coupling makes cross-node transfer, streaming, and speculative execution harder because all dependent state must migrate or be rebuilt coherently.

**Runlith overlap:** `ALREADY_DESIGNED`. Cache/projection/context data is derived and invalidatable; canonical workflow and authority state has independent owners. Provider-native state is not durable Task truth.

**Decision:** `EXTERNAL_VALIDATION`.

### SAS-F05 — Training resume restores operational state, not weights alone

**finding_id:** `SAS-F05`

**source:** implementation

**repository:** parent SAS

**revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

**file / symbol / region:** `src/sas/train/trainer.py`, checkpoint resume `347-379`, checkpoint save `490-507`.

**observation:** checkpoints restore global step, LR scheduler, dataloader state, environment meter, optimizer, and Torch RNG state. The loader snapshots optimizer param-group hyperparameters before DCP load and repairs keys DCP may silently omit.

**confidence:** `HIGH`

**Problem:** restoring only weights can resume at the wrong data position, random state, optimizer schedule, or hyperparameter configuration.

**Mechanism:** persist the execution state needed to continue the same training process and explicitly repair a known persistence mismatch.

**Why it works:** restart semantics are defined by the whole operational state needed for the next step.

**Limitations:** the checkpoint is training-specific; it is not a transaction log for external side effects.

**Runlith overlap:** `ALREADY_DESIGNED` / existing durability principles. Runlith's Attempt/checkpoint/recovery contract is stronger because it must also account for side effects, verification, workspace state, and ambiguity.

**Decision:** `EXTERNAL_VALIDATION`.

### SAS-F06 — Attention budget is a runtime knob with quality/performance tradeoffs

**finding_id:** `SAS-F06`

**source:** implementation

**repository:** parent SAS + pinned SGLang

**revision:** parent `c7d7101...`; SGLang `2a090d5...`

**file / symbol / region:**

- `scripts/eval/_serve.sh`, `20-37`, `156-164`.
- `third_party/sglang-blocksparse/python/sglang/srt/configs/model_config.py`, `328-398`.
- `third_party/sglang-blocksparse/python/sglang/srt/layers/attention/blocksparse/seer_attn/backend.py`, `84-96`.

**observation:** the same trained gate checkpoint can be served with different decode token budgets. SGLang turns the token budget into a block budget at runtime. This creates a direct latency/quality/context-selection tradeoff without retraining the gate.

**confidence:** `HIGH`

**Runlith overlap:** `ALREADY_DESIGNED`. `ModelRuntimeProfile`, hardware/backend fingerprints, context-size performance, explainable RoutePlan, profiler evidence, and Adaptive Model Router already provide the right product-level home for runtime knobs exposed by a local inference adapter.

**Actual gap:** none. If a future local runtime exposes sparse-attention controls, its adapter can surface them as capability/profile evidence without a new core subsystem.

**Decision:** `INSPIRED_ONLY` until a supported local backend exists.

### SAS-F07 — Sparse compute moves the bottleneck into global selection

**finding_id:** `SAS-F07`

**source:** implementation + paper performance analysis

**repository:** parent SAS / linked paper

**revision:** `c7d7101...`; `arXiv:2609.13141v1`

**file / symbol / region:**

- `src/sas/core/kernels/block_scoring.py` — fused block score/log-softmax output across query × block candidates; public API around `468+`.
- `src/sas/core/attn/simple_sparse_attention.py` — threshold/Top-K path, approximately `471-524`.
- `src/sas/tests/test_block_scoring.py`, large 128K smoke case `377-433`.
- supplemental paper §6.3, end-to-end decode efficiency.

**observation:** sparse attention bounds how many real KV blocks the attention kernel reads, but selector scoring still scans candidate block summaries and Top-K must rank them. The paper reports that Top-K grows from 21% of sparse decode work at 8K to 90% at 512K, while selector scoring also grows with context. The authors explicitly identify selection as the next bottleneck.

**confidence:** `HIGH` for mechanism; paper performance numbers are `REPORTED`, not independently reproduced in this review.

**Problem:** removing the dominant cost from one stage can expose a previously secondary global stage whose scaling now dominates total latency.

**Mechanism:** measure the complete optimized path and decompose it by stage rather than reporting only the accelerated kernel.

**Why it works:** optimization decisions remain attached to end-to-end cost instead of local microbenchmark wins.

**Limitations:** exact proportions depend on model, GPU, batch, context, kernel version, and budget.

**Runlith overlap:** `ALREADY_DESIGNED`. `EffectiveContextCost` explicitly includes transformation overhead, latency and recovery risk; the Efficiency Observatory tracks context materialization, cache, wall clock, retries and verification; `Token Cost` and `Attention Load` are intentionally separate.

**Decision:** `EXTERNAL_VALIDATION`.

## Implementation techniques

### Differentiable hard-budget routing

The core training trick keeps discrete Top-K for the actual sparse attention scope while preserving continuous selector scores inside attention logits. The hard selection controls work; the soft score controls gradient. This is a useful example of separating **execution eligibility** from **preference signal** inside one algorithm, but Runlith should use its own typed hard gates and evidence-based preference logic rather than copying the ML mechanism.

### Detached selector inputs with frozen backbone

`SimpleSparseAttention.forward` detaches pre-RoPE query/key states before routing while the trainer marks only `router` parameters trainable. That prevents the sparse-selector objective from mutating backbone representations and keeps the adaptation boundary narrow.

### Fused Triton scoring and sparse attention

The project implements fused block score/log-softmax and sparse-attention forward/backward paths rather than materializing dense token-level attention. The test suite compares kernels against reference implementations for forward/backward correctness, GQA, KV-cache offset semantics, fp16/bf16, edge cases, and large 128K smoke workloads when sufficient CUDA memory is available.

### Incremental block summaries

At serving time a completed logical block stores a compressed key summary. The current partial block keeps only an fp32 `[max|min|sum]` accumulator. This avoids buffering another raw pre-RoPE block per request/layer while still allowing `max|min|avg` reconstruction at completion.

### Explicit export contract

The training exporter validates that the learned gate configuration is one the SGLang backend actually implements. It also repairs the chat template using the base model because a training tokenizer template may omit tool rendering and silently break function calling. This is a concrete example of deployment metadata being part of behavioral compatibility, not decoration.

## AI / orchestration architecture

SAS itself has no durable AI-agent orchestration layer.

### Agent loop

Absent. BFCL/VitaBench execute external benchmark agent loops; SAS serves the model endpoint.

### Planner / subagents / skills / memory

Absent as product concepts.

### Tools

Function/tool use appears only in benchmark evaluation. The gate export explicitly ensures a tools-capable chat template is retained because missing template logic can suppress tool schemas.

### Context

Context selection happens inside model attention at KV-block granularity. It is **not** equivalent to Runlith Context Engine retrieval/materialization: the full KV cache remains available while decode attention reads a selected subset.

### Provider routing / model selection

Absent. Model/backend choices are shell/config parameters.

### Retry/fallback

The HTTP eval client retries any exception up to a finite default (`3`) with linear backoff. There is no typed provider fault classification or provider fallback policy.

### Cost accounting

Eval captures completion tokens when returned by the API, and the paper measures decode latency. There is no Runlith-like Usage Ledger combining provider/runtime cost, retries, verification, and provenance.

### Long-running execution / human approval / completion

Training is checkpointed long-running computation. There is no human approval or product completion authority.

### Answers to mandatory agent questions

| Question | Answer |
|---|---|
| Who owns workflow state? | No product workflow. Trainer owns training progress; benchmark harness owns eval process state. |
| Who owns completion authority? | Training/eval process termination and scorer output; no durable application completion authority. |
| Can the model mutate canonical state? | No application canonical state exists. Model outputs feed benchmark scoring only. |
| Can model output directly cause privileged action? | In SAS itself, no general privileged tool executor. BFCL/VitaBench may exercise benchmark tools under their own contracts. |
| What happens after crash? | Training can resume from DCP checkpoint. Eval/server generally reruns. |
| Provider timeout? | Eval HTTP request is retried up to the finite retry count, then converted into a failure marker string. |
| How does execution resume? | Training restores checkpointed operational state; no durable agent-run resume protocol exists. |

## Security analysis

### SAS-SEC-01 — Serving launcher grants model repository remote-code authority

**classification:** `CONFIRMED`

**source:** `scripts/eval/_serve.sh`, `_sas_serve`, `134-164`, especially `138`.

**observation:** every SGLang launch includes `--trust-remote-code`.

**impact:** if an operator supplies an untrusted remote model/repository that requires custom code, model selection also becomes Python-code execution under the SGLang process authority. In the documented SAS flow the gate/base model is operator-selected and expected to be trusted, so this is a trust-boundary expansion rather than proof of remote exploitation.

**Runlith lesson:** model artifacts/runtime plugins are code/data supply-chain inputs. Local model launch must remain behind Policy, provenance/allowlisting where appropriate, explicit network/filesystem/secrets authority, and Runner isolation. A model name/path cannot implicitly grant host authority.

**Runlith status:** `SUPERSEDED` by the stronger G6 Policy/Runner/Secrets architecture; future local inference adapters must preserve it.

### SAS-SEC-02 — VitaBench secret is materialized to a temporary file

**classification:** `PLAUSIBLE`

**source:** `scripts/eval/run_vitabench.sh`, `48-53`, `75-116`; cleanup registration in `scripts/eval/_serve.sh`, `175-186`.

**observation:** the DeepSeek API key is rendered into a temporary YAML file with mode `0600`; the file is registered for deletion on normal shell exit/INT/TERM.

**positive control:** file permissions are restrictive and results do not intentionally contain the key.

**residual risk:** `SIGKILL`, kernel/host crash, abrupt power loss, or cleanup failure can leave plaintext secret material in the temp directory until external cleanup removes it.

**Runlith lesson:** this is useful hygiene for a research harness, but Runlith's Secret Broker is stronger: secrets should be delivered only to the exact target/operation and should not be persisted to recoverability/state stores unless the integration strictly requires it.

**Runlith status:** `SUPERSEDED` by existing G6 Secrets Broker contracts.

### SAS-SEC-03 — Gate checkpoint unsafe deserialization

**classification:** `NOT_SUPPORTED` for the intended reviewed environment

**source:** `third_party/sglang-blocksparse/python/sglang/srt/models/qwen3_seer.py`, `_load_gate_weights`, `347-356`; parent `pyproject.toml`, `12-16`.

**observation:** the SGLang fork calls `torch.load(pth, map_location="cpu")` without spelling `weights_only=`. The parent project requires `torch>=2.8`; PyTorch 2.6+ defaults `torch.load` to `weights_only=True`. Therefore the reviewed supported dependency floor does not substantiate arbitrary pickle execution through this specific gate load.

**caveat:** pinning the safe dependency floor and making `weights_only=True` explicit would make the security assumption clearer and more robust to alternate environments.

### SAS-SEC-04 — Exported config can contain an absolute host path

**classification:** `PLAUSIBLE` privacy/reproducibility weakness

**source:** `src/sas/train/export_gate.py`, `136-186`, especially `158-162`, `181`.

**observation:** a local `base_model` is resolved to an absolute path and written into exported `config.json`.

**impact:** sharing the exported gate directory can disclose local filesystem layout/usernames and produces an artifact whose base-model reference is host-specific.

**Runlith lesson:** portable execution artifacts should prefer logical/content-addressed resource identity plus separately resolved local location. Host paths are runtime bindings, not portable provenance.

### Supply-chain observations

- README setup recommends `curl -fsSL https://pixi.sh/install.sh | bash` (`README.md:31-35`). This is convenient but executes network-fetched code directly.
- `pyproject.toml` declares VeOmni by Git URL without a revision (`18-21`), while the reviewed `pixi.lock` resolves it to commit `3868ebb5e5fb366c7153d1654f9d2dfc0635c2be`. Reproducibility is therefore stronger when the lockfile is honored than when a resolver consumes only the manifest.
- No root license file or license declaration was found in the reviewed parent revision. This review treats the repository as a knowledge source only, as required by the task; code reuse would require separate license review even aside from Runlith's no-copy rule.

### Security categories without supporting findings

No source evidence in the SAS-owned paths supports claims of exploitable path traversal, SSRF, webhook trust bypass, IPC authentication bypass, application authorization bypass, sandbox escape, or direct shell injection from untrusted remote input. The project is primarily a local research/training harness and does not expose those product surfaces.

## Reliability / recovery analysis

### Strong: operationally complete training checkpoints

`SAS-F05` is the strongest recovery mechanism: data position, scheduler, optimizer, runtime meter and RNG are included, and a known DCP loss mode is repaired explicitly.

### Strong: startup compatibility guards

The inference backend refuses modes that would make auxiliary state unsound. This turns silent quality corruption into an explicit unsupported-capability failure.

### Strong: derived-state allocation fails hard on accounting corruption

If the summary slot allocator unexpectedly exhausts, the SGLang fork raises rather than leaving sentinel slot `0`, because sentinel continuation would silently score real blocks as empty. That is good fail-closed reliability behavior.

### Weak: eval retry failure is encoded as ordinary text

`src/sas/eval/client.py:26-57` returns a string beginning `[REQUEST_FAILED...` after retry exhaustion. This keeps batch order/completion convenient but conflates transport failure with the same value channel as model-generated text. The retry loop also catches every exception and does not record retry count/backoff/latency in a typed result.

Runlith already has the stronger answer: typed Provider/Attempt failures, durable events, bounded retry ownership, and no hidden retry of stateful side effects.

### Weak: process cleanup is heuristic

`scripts/eval/_serve.sh:175-186` kills the tracked child and then also uses `pkill -f "sglang.launch_server.*--port $PORT"`. The latter can match an unrelated same-user server command on the same port. This is acceptable research-harness cleanup but not a production ownership model.

### Weak: evaluation reproducibility is distributed across shell environment

Timestamped result directories help avoid collisions, but a complete benchmark run depends on model path, gate revision, SGLang gitlink, environment variables, runtime YaRN override, hardware, cloud evaluator alias, upstream benchmark checkout/patch, and retry behavior. There is no single immutable run manifest binding all of those values and measured hardware/backend provenance.

Runlith already requires stronger benchmark provenance (`INV-062`, `INV-063`, `INV-080`).

## Performance / scalability analysis

### What the implementation actually scales

- Prefill stays dense.
- Decode reads only a bounded selected KV block set for sparse layers.
- Block summaries are much smaller than full raw block copies.
- Selector scoring still scans candidate summaries.
- Top-K still ranks the candidate set.
- Training's block-score representation remains proportional to query positions × candidate blocks even though token-level dense attention is avoided.

### Paper-reported results, not independently reproduced here

The linked paper reports, under Qwen3-4B/SGLang single-GPU CUDA-graph steady-state decode:

- near parity with dense attention around 8K at batch 1;
- approximately `2.4×`, `4.6×`, and `5.6×` decode speedup at 64K, 256K, and 512K respectively at batch 1;
- roughly `13×` at 64K for batch 8;
- Top-K selection rising to about `90%` of sparse decode cost at 512K.

The paper also reports substantially better quality than SeerAttention-R under tight budgets, including `+6.0–7.7` points on MATH500 and `+10.6–15.5` on GPQA-Diamond at a 1024-token budget across Qwen3 4B/8B/14B. These are external reported measurements and retain the paper's hardware/runtime/evaluation assumptions; they are not local Runlith benchmark evidence.

### Important limitation from the paper

RULER results degrade sharply as context length grows despite SAS improving over SeerAttention-R. At 128K, a large gap remains versus full attention. The authors attribute this mainly to pooled block summaries losing fine-grained/needle-like information. This is strong evidence against treating sparse attention as a universal context-quality replacement.

### Runlith consequence

Do not translate "fewer attended KV tokens" into "smaller context" or guaranteed lower verified cost. For a future local runtime, Runlith should record at least:

- full prompt/context tokens;
- effective attended/sparse budget if observable;
- prefill latency;
- decode throughput/latency;
- selector/ranking overhead;
- memory footprint;
- quality/verification outcome by task class;
- hardware/runtime/build/model/gate revision.

The architecture already has appropriate homes in `ModelRuntimeProfile`, benchmark provenance, Usage Ledger/Efficiency Observatory, and `Token Cost` vs `Attention Load`.

## Anti-patterns

### SAS-AP01 — Transport failure represented as model text

**Evidence:** `src/sas/eval/client.py:26-57`.

**Immediate impact:** scorers/callers need sentinel-string knowledge to distinguish missing inference from genuine generated text; retry metadata is lost.

**Long-term impact:** benchmark accounting can hide transport instability and cannot cleanly reason about retry cost, provider fault class, or partial failure.

**Runlith lesson:** typed provider outcomes and Attempt evidence must remain structurally separate from model content.

**Existing Runlith protection:** `INV-066`, `INV-091`, orchestration/provider fault contracts and durable Attempt outcomes.

### SAS-AP02 — Model artifact trust implicitly widens process code authority

**Evidence:** `scripts/eval/_serve.sh:134-140` unconditionally adds `--trust-remote-code`.

**Immediate impact:** a model repository can become executable code inside the serving process.

**Long-term impact:** model routing/catalog selection can accidentally become an authorization mechanism for code execution if artifact trust and runtime isolation are not independent controls.

**Runlith lesson:** model/provider selection is routing evidence; process/code authority is Policy + Runner authority.

**Existing Runlith protection:** G6 capability/Policy/Runner boundaries, provider environment isolation, Secrets Broker, `INV-060`.

### SAS-AP03 — Heuristic framework monkey-patching as a portability boundary

**Evidence:** `src/sas/core/patch/hf.py:33-55` scans module names for `self_attn`/`attention`, checks for projection-like attributes, and mutates parent modules in place.

**Immediate impact:** compatibility depends on framework/model naming and structure conventions that are not expressed as a versioned adapter contract.

**Long-term impact:** upstream model refactors can silently miss or misidentify patch targets; testing burden grows with model families.

**Runlith lesson:** keep provider/runtime-specific adaptation behind explicit adapter contracts and capability probes. Do not put naming heuristics into canonical domain logic.

**Existing Runlith protection:** provider ACL/adapter architecture and capability-driven routing.

### SAS-AP04 — Optimizing one expensive stage without accounting for the selector

SAS itself actually avoids this anti-pattern in its paper analysis by measuring the selector/Top-K breakdown. The failure mode is still worth recording as a general lesson because the implementation demonstrates how easily it can emerge: sparse attention becomes bounded while global ranking still scales with context.

**Runlith lesson:** every optimization must include selection/transformation/index/recovery overhead in the end-to-end cost and quality metric. This is already encoded in `EffectiveContextCost` and `VerifiedDeliveryCost`.

## Runlith overlap analysis

### External idea: optimize selection against downstream outcome

**External evidence:** `SAS-F01`.

**Closest Runlith equivalent:** `VerifiedDeliveryCost`, Efficiency Observatory, local verified routing feedback.

**Runlith evidence:** Source of Truth `2000-2044`, `2447-2476`, Adaptive Model Router `5159-5181`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: separate raw context size from attention work

**External evidence:** fixed sparse decode budget with full KV retained; `SAS-F06/F07`.

**Closest Runlith equivalent:** explicit `Token Cost` vs `Attention Load`; `EffectiveContextCost`.

**Runlith evidence:** Source of Truth `2335-2349`, `2664-2671`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: runtime budget as a profiled inference knob

**External evidence:** `SGLANG_SEER_TOKEN_BUDGET` changes block budget without retraining.

**Closest Runlith equivalent:** `ModelRuntimeProfile`, `ModelProfilerPort`, context-size performance, explainable model routing.

**Runlith evidence:** Source of Truth `4956-5203`; `INV-059..064`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** no core gap. Adapter-specific support belongs to a future local inference backend when one exposes such a control.

**Decision:** `INSPIRED_ONLY`.

### External idea: fail closed on incompatible optimized execution modes

**External evidence:** `SAS-F03`, SGLang backend guards.

**Closest Runlith equivalent:** hard eligibility gates before routing preference; mandatory isolation/capability failures fail closed.

**Runlith evidence:** Source of Truth `4960-4964`, `5090-5124`; `INV-060`; G6 Policy/Runner architecture.

**Current status:** `ALREADY_DESIGNED` / security enforcement principles `ALREADY_IMPLEMENTED`.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: derived optimization cache follows owner lifecycle

**External evidence:** `SAS-F04` summary slots + rolling accumulator.

**Closest Runlith equivalent:** derived cache/context state is invalidatable and non-authoritative; provider/session native state does not own workflow continuity.

**Runlith evidence:** Context/continuity architecture, cache invalidation rules, provider/session hierarchy, `INV-019`, `INV-022`, `INV-023`, `INV-066`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: complete operational checkpoint state

**External evidence:** `SAS-F05`.

**Closest Runlith equivalent:** durable Attempt/checkpoint/recovery semantics, provider-independent continuity, workspace/Git state ownership.

**Runlith evidence:** existing G3-G5 durability/recovery architecture and `INV-066`.

**Current status:** `ALREADY_DESIGNED` with substantial implementation already present for current lifecycle scopes.

**Actual gap:** none derived from SAS.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: retry any eval HTTP error and return a sentinel string

**External evidence:** `src/sas/eval/client.py:30-57`.

**Closest Runlith equivalent:** typed ProviderFault/Attempt outcome and bounded side-effect-aware retry policy.

**Runlith evidence:** AGENT.md retry rules; `INV-091`; app-service orchestration tests explicitly reject hidden retries.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE` as a production Runlith pattern.

**Actual gap:** none.

**Decision:** `REJECT`.

### External idea: always trust remote model code in serving launcher

**External evidence:** `scripts/eval/_serve.sh:134-140`.

**Closest Runlith equivalent:** Policy-controlled process/network/filesystem/secret authority + Runner isolation.

**Runlith evidence:** G6 architecture and implemented security services/runner boundaries.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE` if copied literally.

**Actual gap:** none.

**Decision:** `REJECT`.

## External validation of existing Runlith design

SAS materially validates these existing Runlith decisions:

1. **Final outcomes beat proxy metrics.** Selector quality is better judged by downstream behavior than by reproducing a proxy distribution. Runlith's analogue is verified delivery rather than token/model/benchmark vanity metrics.
2. **Hard eligibility precedes preference.** An otherwise attractive optimized backend is rejected if it cannot preserve required state semantics.
3. **Derived state needs explicit ownership and invalidation.** Auxiliary summary state is tied to request/KV lifecycle and cannot become a second truth.
4. **Token count and attention/runtime cost are different dimensions.** A system can keep the full logical context while limiting attention reads; selector cost can still dominate.
5. **Optimization must be measured end to end.** Kernel savings alone are incomplete once selection/ranking becomes the new bottleneck.
6. **Runtime evidence is hardware/backend/config specific.** Sparse budget, batch, context, CUDA graph mode and serving backend materially change results.
7. **Compatibility metadata affects behavior.** Chat template, base model identity, gate configuration and runtime backend are all part of serving correctness.

## Novel gaps

**None found at the Runlith architecture level.**

The most tempting candidate would be "add sparse-attention budget to model routing," but that is already representable by the existing `ModelCapabilities` / `ModelRuntimeProfile` / `ModelProfilerPort` / adapter model. Creating a new subsystem or canonical task now would duplicate the existing architecture without a concrete supported local backend.

The correct future action, if a Runlith local inference adapter gains SGLang/llama.cpp/MLX or another backend with comparable attention controls, is to benchmark that backend under the existing profiling and routing contracts. That is ordinary adapter/profile work, not a SAS-derived architecture gap today.

## Adoption matrix

| External idea | Runlith overlap | Decision | Reason |
|---|---|---|---|
| Train/learn selector against final downstream objective | `ALREADY_DESIGNED` analogue | `EXTERNAL_VALIDATION` | Reinforces verified-outcome optimization instead of proxy metrics. |
| Frozen base + small independently deployed learned delta | lower-level runtime technique | `INSPIRED_ONLY` | Useful for model adapters, not a Runlith core pattern by itself. |
| Fail-closed optimized-backend compatibility guards | `ALREADY_DESIGNED` / security principle partly implemented | `EXTERNAL_VALIDATION` | Matches hard eligibility before preference. |
| Co-lifecycle derived summary/cache state | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Matches derived-cache/non-authoritative state ownership. |
| Runtime sparse attention budget | `ALREADY_DESIGNED` generic runtime profiling | `INSPIRED_ONLY` | Expose only through a backend that actually supports it. |
| Complete operational checkpoint state | existing durability/recovery model | `EXTERNAL_VALIDATION` | Same principle; Runlith needs a broader side-effect-aware contract. |
| Return transport failure as model-content string | typed failure already exists | `REJECT` | Loses fault/retry provenance and conflates data with execution state. |
| Unconditional `--trust-remote-code` | G6 authority model | `REJECT` | Model artifact selection must not silently widen process authority. |
| HF module-name heuristic monkey patch as core integration contract | provider adapter architecture | `REJECT` | Too framework/model-structure dependent for canonical domain logic. |
| Sparse compute without selector-overhead accounting | `VerifiedDeliveryCost`/`EffectiveContextCost` | `EXTERNAL_VALIDATION` of the risk | SAS paper itself demonstrates the bottleneck shift. |

## Ideas rejected for Runlith

### Put sparse-attention semantics into the Context Engine

Rejected. Context Engine decides what evidence is selected/materialized for a model. SAS decides which already-encoded KV blocks a model attends to during decode. These are different layers with different provenance and correctness semantics.

### Make attention budget a new canonical Task field

Rejected. It is a backend/model runtime knob. If supported, it belongs behind the local inference adapter and runtime profile/routing evidence.

### Copy the Hugging Face monkey-patching model adaptation

Rejected. It is appropriate experimental framework glue, but Runlith provider/runtime integration should remain explicit and contract-tested behind adapters.

### Adopt eval retry-as-string handling

Rejected. Runlith already has stronger typed retry/failure semantics.

### Treat paper benchmark numbers as Runlith routing truth

Rejected. They are external evidence tied to specific Qwen3/SGLang/GPU/budget/workload conditions. Runlith requires local/hardware/backend/time provenance and increasingly prefers verified project-local outcomes.

## Candidate ADRs

None.

No reviewed finding requires a new Runlith architecture owner or unresolved cross-boundary contract.

## Candidate implementation tasks

None.

No `NOVEL_GAP`, `PARTIALLY_EXISTS`, or `NEEDS_ADR` finding from this repository justifies a new `TASK-CANDIDATE`. Future local-model backend work may use this report as implementation/benchmark evidence under already-existing runtime profiling and routing architecture.

## Source evidence index

### Parent repository — `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

| Finding / subject | Implementation evidence |
|---|---|
| Project method | `README.md:9-26` |
| Setup / supply-chain install path | `README.md:28-35` |
| Qwen3 training scope/resource claim | `README.md:38-59` |
| Eval architecture / sparse budget | `README.md:61-78` |
| BFCL/VitaBench external harnesses | `README.md:99-145` |
| Router-only training | `src/sas/train/trainer.py:57-61,222-247,441-447` |
| Complete checkpoint save/resume | `src/sas/train/trainer.py:347-379,490-507` |
| Router-only final export | `src/sas/train/trainer.py:534-550` |
| Sparse attention architecture / gating / Top-K | `src/sas/core/attn/simple_sparse_attention.py:295-340,403-530` |
| Router detached Q/K inputs | `src/sas/core/attn/simple_sparse_attention.py:429-431` |
| Export compatibility validation | `src/sas/train/export_gate.py:41-71` |
| Tool-capable chat-template repair | `src/sas/train/export_gate.py:92-133` |
| Absolute base path + gate artifact export | `src/sas/train/export_gate.py:136-201` |
| Heuristic HF patch boundary | `src/sas/core/patch/hf.py:33-55` |
| Eval bounded concurrency/retry/failure sentinel | `src/sas/eval/client.py:26-78,95-156` |
| Serve config, remote-code trust, sparse budget | `scripts/eval/_serve.sh:60-98,103-164` |
| Serve cleanup/readiness | `scripts/eval/_serve.sh:167-204` |
| VitaBench secret temp file | `scripts/eval/run_vitabench.sh:48-116` |
| Parent dependency floor / VeOmni direct ref | `pyproject.toml:5-24` |
| Exact third-party gitlinks | `.gitmodules:1-12` plus `git ls-tree HEAD third_party/*` |
| Kernel KV-cache/reference tests | `src/sas/tests/test_block_scoring.py:268-375`; `src/sas/tests/test_sparsex_attn.py:362+` |
| 128K kernel smoke coverage | `src/sas/tests/test_block_scoring.py:377-433`; `src/sas/tests/test_sparsex_attn.py:262-326` |

### Pinned SGLang inference submodule — `2a090d525cc52dc8eb70059543271baef34c5696`

| Finding / subject | Implementation evidence |
|---|---|
| Gate/base model split and gate weight load | `python/sglang/srt/models/qwen3_seer.py:295-358` |
| Gate configuration validation / runtime overrides | `python/sglang/srt/configs/model_config.py:328-404` |
| Sparse budget derivation and runtime compatibility guards | `python/sglang/srt/layers/attention/blocksparse/seer_attn/backend.py:84-180` |
| Derived summary slot lifecycle / fail-hard exhaustion | `python/sglang/srt/mem_cache/blocksparse_summary.py:3-14,35-122,175-200` |
| Summary + rolling accumulator representation | `python/sglang/srt/mem_cache/seer_attn_memory_pool.py:3-31,56-129` |

### Supplemental paper evidence — `arXiv:2609.13141v1`

- method/ablation: continuous inner-softmax gating and sparse/full training scope;
- §5: reasoning, LongBench, BFCL, VitaBench evaluations;
- §6.3: end-to-end decode latency and selector/Top-K cost breakdown;
- Appendix B: RULER long-context degradation and block-summary limitation.

Paper metrics in this report are labeled **reported** and were not independently reproduced during this architecture review.

## Review conclusion

SAS is high-quality implementation intelligence for Runlith's future local-model runtime work, especially because it exposes both the benefit and the hidden cost of learned sparse context selection. Its deepest architectural lesson is already present in Runlith: optimize the verified outcome, keep hard eligibility separate from preference, treat optimization caches as derived state, and measure the complete cost path with provenance.

The review therefore records external validation and reusable patterns without expanding the Runlith roadmap.
