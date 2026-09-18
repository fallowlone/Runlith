# The Ultra-Scale Playbook

## Source revision

- **Primary source:** `https://huggingface.co/spaces/nanotron/ultrascale-playbook`
- **Repository type:** Hugging Face Space backed by Git. The supplied PDF is a rendered artifact of this repository, not a separate canonical source repository.
- **Repository name:** `nanotron/ultrascale-playbook`
- **Default branch:** `main`
- **HEAD reviewed:** `c93890dd4b22356ee18ed9b863e6f2d0909729bc`
- **Tag/version:** no tag at reviewed HEAD. The PDF metadata reports creation on 2026-09-15; findings are bound to the Git revision above rather than to PDF generation time.
- **Review date:** 2026-09-15
- **License metadata:** Apache-2.0 in the Space README frontmatter.
- **Supplied artifact:** `/Users/artemmac/Downloads/Ultra-Scale_Playbook_for_LLM_Training.pdf` (246 pages).
- **Production implementation reference:** `https://github.com/huggingface/nanotron`, reviewed at `2411b022a75fb7f7561a1bb4166706da5e1b76de` (`main`, package version `0.4` in `pyproject.toml`).
- **Educational implementation reference:** `https://github.com/huggingface/picotron`, reviewed at `59714b1bf25541dbeb6f5a721ed15f6d0768e786` (`main`).

The Playbook itself is primarily a static research/book repository. Where a finding concerns runtime behavior rather than the book's benchmark evidence or architectural explanation, this report uses Nanotron source as implementation evidence. No external source code is copied into Runlith and no external repository is added as a dependency.

## Executive summary

The strongest lesson for Runlith is not a GPU-specific parallelism algorithm. It is the Playbook's **measurement-driven resource orchestration discipline**: model workload, memory capacity, communication cost, physical topology, overlap opportunities, and contention are treated as separate constraints; candidate configurations are narrowed analytically and then validated empirically on the actual hardware.

That direction substantially validates Runlith's existing Adaptive Model Router, `ModelRuntimeProfile`, `BenchmarkEvidencePort`, `DeviceResourceManagerPort`, `ModelResidencyManagerPort`, reproducible benchmark provenance, and multi-objective verified-delivery-cost model. Runlith has already designed the correct higher-level separation between policy/routing decisions and platform-specific resource observation.

One material gap remains. Runlith currently models device capacity and runtime performance but does not yet define an explicit provider-neutral **hardware-topology / interconnect / contention evidence contract**. The Playbook repeatedly demonstrates that the same nominal resources can behave very differently depending on locality (for example intra-node versus inter-node links), communication topology, and concurrent use of compute resources. This becomes relevant when Runlith reaches multi-device local inference or G12 remote-worker placement. It should be handled as a future ADR/research item, not as a current G7 implementation task.

The most useful negative evidence is equally strong. Nanotron contains concrete examples of failure modes Runlith already tries to exclude: a background checkpoint mover can remain forever in a non-idle state after a failed subprocess; evaluation scripts recover a Hugging Face token from the user's home directory and export it into a spawned job; helper paths interpolate strings into shell commands; checkpoint/cache code uses pickle-backed loading. These are useful external validation for Runlith's bounded liveness, Secrets Broker, governed Runner, environment isolation, path authority, and untrusted-artifact rules.

## Project purpose

The Ultra-Scale Playbook documents how to train large language models efficiently from one GPU to large clusters. It combines analytical memory/communication models with thousands of measured scaling experiments. The source explicitly argues that there is no universal parallelism recipe because the best configuration depends on both model properties and concrete infrastructure.

Nanotron is the production-oriented training framework used as the main implementation reference. Its `DistributedTrainer` builds process groups, model, optimizer, scheduler, checkpoint/recovery state, pipeline engine, metrics, and training loop. It supports combinations of data, tensor, pipeline, context, and expert parallelism, plus checkpointing and profiling.

Picotron is positioned by the Playbook as a smaller educational reference. It was revision-frozen for provenance but did not materially change the Runlith conclusions, so source-level runtime findings below prefer Nanotron.

## Architecture map

```text
Ultra-Scale Playbook
├── Interfaces
│   ├── static web/PDF book
│   ├── diagrams and benchmark visualizations
│   └── benchmark/result references
├── Orchestration
│   └── conceptual composition of DP / TP / PP / CP / EP / ZeRO
├── Domain
│   └── distributed LLM training performance model
├── State/Persistence
│   └── none in the static book itself
├── Providers
│   └── PyTorch / NCCL / CUDA / Triton / cluster runtime references
├── Tools/Capabilities
│   └── profiler, benchmark scripts, cluster launch tooling
├── Execution
│   └── delegated to Nanotron/Picotron and underlying GPU runtimes
├── Recovery
│   └── discussed indirectly; concrete checkpoint/restart behavior lives in Nanotron
├── Security
│   └── not a primary design subject of the book
└── Observability
    ├── PyTorch Profiler
    ├── CUDA timing
    ├── Nsight / NCCL evidence
    └── reproducible benchmark datasets
```

The companion Nanotron runtime maps more concretely as:

```text
Nanotron
├── Interface / entrypoints
│   └── run_train.py, run_generate.py, run_evals.py, slurm_launcher.py
├── Lifecycle owner
│   └── DistributedTrainer
├── Parallel topology
│   └── ParallelContext + TP/PP/DP/CP/EP process groups
├── Scheduling
│   └── PipelineEngine (AFAB / 1F1B) + distributed collectives
├── State
│   ├── Config
│   ├── TrainingMetadata
│   ├── optimizer / LR scheduler / random state
│   └── checkpoint metadata + latest.txt
├── Persistence / recovery
│   ├── safetensors model shards
│   ├── optimizer/scheduler/random-state shards
│   ├── checkpoint metadata version
│   └── S3Mover for asynchronous checkpoint transfer
├── Execution backends
│   └── PyTorch distributed / NCCL / CUDA
└── Observability
    ├── rank-aware logs
    ├── CUDA event timers
    ├── profiler traces
    └── benchmark CSV/config provenance
```

## Canonical state ownership

### Playbook

The static Playbook has no runtime canonical mutable state. Its architectural authority is explanatory/benchmark evidence in `ultra_blog.md` plus referenced benchmark datasets and implementation repositories.

### Nanotron

For a live training process, `DistributedTrainer` is the lifecycle owner. It constructs `ParallelContext`, model, optimizer, scheduler, checkpoint state, pipeline engine, and the step loop (`src/nanotron/trainer.py:131-298`, `519-602`).

Durable restart state is split across checkpoint artifacts. `TrainingMetadata` persists the last training step, consumed samples/tokens, and stage metadata (`src/nanotron/serialize/metadata.py:18-95`). Model, optimizer, LR scheduler, random states, configuration, and metadata are serialized separately; `latest.txt` points to the most recent completed checkpoint (`src/nanotron/trainer.py:1244-1285`). `parse_ckpt_path` restores the checkpoint selected by that pointer (`src/nanotron/serialize/main.py:207-275`).

There is no product-style Task/Execution/Attempt state machine. Pipeline state is process-local execution state for a batch. This is appropriate for a training framework but must not be confused with Runlith's durable orchestration model.

## Strong engineering decisions

### US-PAT-001 — Treat resource optimization as a constrained, hardware-specific search problem

**Source:** Playbook `ultra_blog.md`, sections “High level overview”, “Combining the five parallelism axes”, and “Finding the best training configuration”, especially regions `104-128`, `2008-2027`, `2117-2197`.

**Observation:** The book separates memory, compute efficiency, and communication overhead, then ties the best strategy to actual interconnect, GPUs per node, model dimensions, and memory. It explicitly rejects a universal configuration.

**Why it works:** It avoids collapsing qualitatively different bottlenecks into one score. A configuration is first made feasible, then optimized for throughput under real hardware constraints.

**Limitations / failure modes:** A model can be analytically plausible and still lose on real hardware because implementation quality, allocator behavior, kernel scheduling, link contention, or framework details differ from the approximation.

**Complexity:** Medium at the modeling layer; high if extended to automatic fleet placement.

**Runlith overlap:** `ALREADY_DESIGNED` at the model/provider routing level. `ModelRuntimeProfile`, `ProviderCapacityProfile`, `BenchmarkEvidencePort`, hard eligibility gates, and versioned `RoutePlan` inputs already encode the same separation. The missing part is explicit physical-topology/contended-resource evidence for future local/remote placement.

**Decision:** `EXTERNAL_VALIDATION` for current routing architecture; `ADAPT` for future topology-aware placement.

### US-PAT-002 — Analytic preflight before benchmark search, empirical evidence before promotion

**Source:** Playbook `ultra_blog.md:2117-2251` and Appendix A3 `3777-3908`; Nanotron `scripts/scaling_benchmarks.py:18-115`, `177-224`, `227+`.

**Observation:** The Playbook derives memory and compute/communication ratios to narrow the configuration space, then benchmarks combinations and records configuration provenance/results. It also shows that implementation changes can change the winning configuration.

**Why it works:** Cheap reasoning removes impossible or obviously poor candidates; measurement handles the residual system effects that analytic models miss.

**Limitations:** Benchmark data expires when hardware, model, kernel, runtime, or topology changes. Benchmarking itself can be expensive and failure-prone.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith already has hard feasibility gates before scoring, `ModelProfilerPort`, `BenchmarkEvidencePort`, runtime/hardware fingerprinted profiles, measured timestamps/confidence, and benchmark non-regression/provenance invariants.

**Decision:** `EXTERNAL_VALIDATION`.

### US-PAT-003 — Place tightly coupled communication on the fastest locality boundary

**Source:** Playbook TP/SP and 5D sections, especially `1371-1410` and `2008-2027`.

**Observation:** Tensor-parallel work that depends on frequent collectives is kept within high-bandwidth intra-node links when possible; slower inter-node boundaries are assigned strategies with less critical-path communication. The reported benchmark cliff at TP=8 to TP=16 is tied to crossing from intra-node NVLink to slower inter-node communication.

**Why it works:** Nominal GPU count does not describe communication cost. Placement must respect the physical communication graph.

**Limitations:** The exact TP/PP rule is GPU-training-specific. Runlith must not import those axes into its domain model.

**Runlith overlap:** `PARTIALLY_EXISTS`. Runlith already isolates device/resource observation behind ports and plans G12 workers with health/capacity, but no explicit interconnect/topology contract was found in architecture docs or implementation.

**Decision:** `ADAPT` as provider-neutral `HardwareTopologyProfile` / placement evidence for future multi-device and G12 scheduling.

### US-PAT-004 — Communication/computation overlap is an optimization hypothesis, not free capacity

**Source:** Playbook `ultra_blog.md:1469-1487`, `2290-2301`, Appendix A3 `3777-3908`.

**Observation:** The book repeatedly overlaps communication with compute, but its benchmark postmortem notes that NCCL work can consume GPU SM resources and therefore interfere with “overlapped” compute. The effective overlap must be measured.

**Why it works:** It prevents optimistic scheduling from double-counting the same underlying resource.

**Limitations:** Contention depends on backend/hardware/runtime and is hard to predict with one static coefficient.

**Runlith overlap:** `PARTIALLY_EXISTS`. Runlith profiles concurrency behavior and uses measured runtime profiles, but has no explicit contention/interference evidence type for resource placement.

**Decision:** `ADAPT` into the same future topology/resource evidence contract rather than a separate subsystem.

### US-PAT-005 — Materialize expensive state only when needed, release it quickly, prefetch the next dependency

**Source:** Playbook ZeRO-3 discussion `ultra_blog.md:794-1012`, especially on-demand parameter gathering/release and next-layer prefetch around `992-999`; FlashAttention discussion `2753-2808` provides a lower-level version of the same principle.

**Observation:** Memory pressure is reduced by keeping only necessary working state resident and prefetching the next required shard while current work executes. FlashAttention similarly avoids materializing large intermediates in slow/global memory.

**Why it works:** It optimizes data movement and peak working set, often the real bottleneck.

**Runlith overlap:** `ALREADY_DESIGNED`. Context Engine progressive disclosure, resource residency management, exact retrieval, content-addressed evidence, and deferred context/tool loading embody the same higher-level principle.

**Decision:** `EXTERNAL_VALIDATION`.

### US-PAT-006 — Version and validate durable restart metadata independently of runtime topology

**Source:** Nanotron `src/nanotron/serialize/main.py:30-43`; `src/nanotron/serialize/metadata.py:88-119`, `160-195`; optimizer reload/resharding in `src/nanotron/serialize/optimizer.py:184-357`.

**Observation:** Nanotron explicitly wants serialized state capable of recovering training and increasingly topology-agnostic loading. Checkpoint metadata is versioned, strict tensor metadata describes unsharded shape and shard slices, and optimizer loading can merge/reshard across topology changes.

**Why it works:** Durable state describes semantic tensor identity/shape independently from one process layout, reducing coupling between persistence and execution topology.

**Limitations:** The source still contains topology assumptions for random-state restoration and comments describing incomplete topology independence (`src/nanotron/serialize/random.py:27-36`).

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith's durable Task/Execution/Attempt state, versioned contracts, provider-independent continuity, and replaceable execution backends already require persistence to outlive one provider/runtime placement.

**Decision:** `EXTERNAL_VALIDATION`.

### US-PAT-007 — Profile asynchronous systems with semantics-aware timing

**Source:** Playbook Appendix A1 `ultra_blog.md:3624-3718`; Nanotron README `178` and trainer timing/profiler paths around `src/nanotron/trainer.py:546-557`.

**Observation:** Host timing around asynchronous GPU launch is misleading. The Playbook uses warm-up, CUDA events, synchronization where needed, and profiler traces to measure the operation that actually matters.

**Runlith overlap:** `ALREADY_DESIGNED` by `INV-063`/`INV-080`: performance evidence must retain hardware/backend/time/build provenance and use reproducible methodology. The general Runlith lesson is to define timing boundaries by lifecycle semantics rather than wall-clock wrappers around asynchronous dispatch.

**Decision:** `EXTERNAL_VALIDATION`.

## Implementation techniques

### Explicit multidimensional process groups

Nanotron's `ParallelContext` creates a deterministic rank matrix and explicit process groups for TP/CP/DP/PP/EP (`src/nanotron/parallel/context.py:12-140`). This makes each communication scope concrete instead of deriving it ad hoc throughout the code.

For Runlith, the transferable principle is scoped resource/coordination groups with explicit membership and purpose. The GPU rank matrix itself is not relevant to the Task domain.

### Replaceable pipeline schedule behind one engine interface

`PipelineEngine` defines the batch schedule contract and concrete AFAB/1F1B engines implement different ordering while sharing forward/backward primitives (`src/nanotron/parallel/pipeline_parallel/engine.py:24-131`, `175-348`).

The useful Runlith analogy is narrow: execution policy can vary behind a stable orchestration contract. It does **not** justify pipelining across Policy, Verification, or completion-authority boundaries.

### Conservative defaults before advanced overlap

`ParallelismArgs.__post_init__` defaults to AFAB, all-reduce, and async TP communication disabled (`src/nanotron/config/parallelism_config.py:41-53`). This reflects a useful operational posture: start with easier-to-reason-about behavior and enable more aggressive overlap deliberately.

### Benchmark records carry configuration identity

The scaling benchmark generator writes model dimensions, DP/TP/PP, microbatching, sequence length, ZeRO stage, job id, and timestamp into experiment records (`scripts/scaling_benchmarks.py:177-224`). This is useful provenance discipline even though Runlith needs a different schema.

## AI / orchestration architecture

This repository is about training AI models, not orchestrating AI agents. It has no planner, subagent system, tool-selection model loop, prompt memory, human approval system, completion judge, or provider-routing authority comparable to Runlith.

For the mandatory orchestration questions:

- **Who owns workflow state?** In Nanotron, `DistributedTrainer` owns live training control; checkpoint metadata owns restart progress.
- **Who owns completion authority?** The deterministic training loop exits after configured training steps; no model output can mark training complete.
- **Can the model mutate canonical workflow state?** No. The trained model participates in numerical computation, not lifecycle decisions.
- **Can model output directly cause privileged action?** No comparable agent/tool pathway exists in the reviewed training loop.
- **What happens after crash?** Progress can resume from the checkpoint selected by `latest.txt`, provided the relevant checkpoint components are valid and compatible.
- **What happens after provider timeout?** There is no LLM-provider abstraction. Distributed/cluster failures are infrastructure failures.
- **How does execution resume?** Reconstruct trainer state from config/checkpoint metadata, model/optimizer/scheduler/random state, then continue from `last_train_step + 1` (`src/nanotron/trainer.py:239-276`, `519-543`).

## Security analysis

The Playbook is not a security-oriented repository. Security findings below come from the production companion Nanotron implementation because the user-supplied source explicitly presents it as the production reference.

### US-SEC-001 — Ambient Hugging Face credential injection into generated evaluation jobs

**Classification:** `CONFIRMED`

**Source evidence:** Nanotron `src/nanotron/eval/one_job_runner.py:163-171` reads `~/.cache/huggingface/token` when `HUGGING_FACE_HUB_TOKEN` is absent and exports it into the Slurm job environment. The same generated script performs network/storage work afterward.

**Impact:** The job receives ambient user credential authority that is not expressed as a narrow, task-scoped capability. Compromise or unintended subprocess behavior inside the job inherits that token.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE` and is already protected against by the G6 Secrets Broker/provider-env-isolation direction plus G12's explicit “no ambient organization secrets” rule.

**Decision:** `REJECT`; use only as negative evidence.

### US-SEC-002 — Shell command construction from path/config strings

**Classification:** `PLAUSIBLE`

**Source evidence:** `scripts/weka.py:105-115` constructs a shell string containing `folder_path` and executes it with `shell=True`; `src/nanotron/eval/one_job_runner.py:145-283` builds a Slurm shell script by interpolating multiple configuration/path fields, including unquoted command arguments at several call sites. `examples/bench_llama_7b.py:86-95` also uses `os.system` for launch.

**Impact:** If any interpolated value crosses a trust boundary, shell metacharacters can become command syntax. The reviewed code does not establish that these fields are attacker-controlled, so exploitability is not asserted.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE`. Runlith's governed Runner, typed argv/environment construction, Policy, and untrusted-input rules are the correct protection.

**Decision:** `REJECT` as an implementation pattern.

### US-SEC-003 — Pickle-backed checkpoint/cache deserialization

**Classification:** `PLAUSIBLE`

**Source evidence:** optimizer/lr-scheduler state uses `torch.load` without an explicit restricted loading contract (`src/nanotron/serialize/optimizer.py:206-211`, `332-369`); random state is persisted/loaded through `torch.save`/`torch.load` with an explicit source TODO noting pickle (`src/nanotron/serialize/random.py:10-36`); S3 file-list cache uses `pickle.load` (`src/nanotron/data/tokenized_bytes.py:324-418`).

**Impact:** If a checkpoint/cache file can be replaced by an untrusted principal, deserialization can cross a code/data trust boundary. The reviewed system appears to assume operator-controlled training artifacts, so this is classified as plausible rather than a proven remotely exploitable vulnerability.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE` for untrusted artifacts. Runlith should keep typed/data-only formats for untrusted persisted evidence and treat executable/provider-native serialization as trusted-runtime state with an explicit trust boundary.

**Decision:** `REJECT` for Runlith canonical persistence.

### US-SEC-004 — Destructive cleanup accepts a broad local path with warning-only placement check

**Classification:** `PLAUSIBLE`

**Source evidence:** `S3Mover.__init__` accepts `local_path`; a path outside `/scratch/` only produces a warning (`src/nanotron/s3_checkpoints/s3_mover.py:70-114`). Cleanup enumerates child directories under that path and invokes `rm -rfv` on them (`333-350`). Cleanup can also be requested on initialization through `clean_up_local_on_start` (`80`, `112-114`).

**Impact:** A trusted-operator misconfiguration can expand deletion authority far beyond an intended checkpoint directory. This is an authority/path-validation weakness; no malicious input path is required for the operational failure mode.

**Runlith overlap:** Existing workspace ownership/path-security/isolation work is stronger. Destructive filesystem actions must be constrained to canonical owned roots and fail closed on path identity/scope mismatch.

**Decision:** `REJECT` as a pattern; `EXTERNAL_VALIDATION` of Runlith's workspace/path authority design.

### NOT_SUPPORTED classes

No source evidence was found in the reviewed Playbook/Nanotron scope for an AI-agent authorization bypass, provider-driven Task-state mutation, prompt-injection authority escalation, browser SSRF surface, IPC authentication bypass, or sandbox escape. Those categories are not applicable or not supported by this source revision.

## Reliability / recovery analysis

### Checkpoint restart has useful semantic state separation

Nanotron checkpoints model weights, optimizer state, scheduler, random states, config, and training metadata separately, then use a step pointer for restart. This is a good example of persisting the state required to continue a deterministic computation rather than relying on one opaque process snapshot.

The design also attempts topology-agnostic optimizer restoration by storing shard metadata and rebuilding shards for a changed parallel topology. That is strong evidence for Runlith's broader rule that durable continuity should outlive one executor layout.

### US-REL-001 — Failed asynchronous checkpoint transfer can become permanently non-terminal

**Classification:** `CONFIRMED`

**Source evidence:** `S3Mover.wait_for_completion` loops until state becomes `IDLE` (`src/nanotron/s3_checkpoints/s3_mover.py:194-197`). `update()` checks the subprocess return code; when it is non-zero it logs and returns the current state without resetting state/process or transitioning to a terminal failure (`373-393`). `distributed_wait_for_completion` similarly polls until all ranks report completion (`199-228`).

**Failure mode:** A completed subprocess with a non-zero exit code remains represented as `UPLOADING`, `DOWNLOADING`, or `REMOVING_CHECKPOINT`. Subsequent polling sees the same failed return code and never reaches `IDLE`, so blocking callers can wait indefinitely.

**Runlith lesson:** Every autonomous/background operation needs explicit terminal failure, bounded waiting, durable failure evidence, and an exhaustion/escalation destination. Runlith already states these principles through liveness/retry invariants and should preserve them in G11/G12 background/remote work.

**Runlith status:** `ALREADY_DESIGNED`; no new candidate task from this source.

### Global barriers amplify stragglers

The Playbook explicitly explains that a barrier stalls every participant until the slowest arrives (`ultra_blog.md:3576-3603`). Nanotron uses barriers during process-group creation, shutdown, and checkpoint coordination. This is correct where global consistency requires it, but it reinforces the importance of keeping synchronization scopes as small as correctness allows.

Runlith's Task DAG / minimal-blocking HumanDecisionRequest direction is consistent with this: unrelated work should not be globally blocked by one slow dependency.

## Performance / scalability analysis

The Playbook's main performance model can be summarized as:

1. establish feasibility under memory constraints;
2. minimize communication on slow links;
3. overlap independent communication and compute where measurements justify it;
4. reduce bubbles/idle time without creating an unacceptable working-set or coordination cost;
5. use profiler evidence to locate real bottlenecks;
6. benchmark the actual model/runtime/hardware combination because implementation details can reverse theoretical rankings.

Important scalability lessons for Runlith:

- **Locality is part of capacity.** Two workers/devices with equal nominal compute are not equivalent if the link between dependent stages differs materially.
- **Concurrency is not additive capacity.** Shared CPU, GPU, memory bandwidth, disk, network, provider quota, or runtime slots can create interference.
- **Microbatching/pipelining trades idle time for state and coordination.** This maps to Runlith only inside safe execution scopes, not across authority/verification boundaries.
- **Global synchronization is expensive.** Prefer dependency-scoped coordination where semantics allow it.
- **Measurement provenance expires.** Hardware/runtime/build/model changes require profile invalidation or confidence decay.

## Anti-patterns

### AP-US-001 — Treat nominal resource count as sufficient placement information

**Evidence:** Playbook `ultra_blog.md:1371-1410`, `2008-2027`, `2117-2197`.

**Immediate impact:** A scheduler can place a communication-heavy workload across a slow boundary and lose throughput despite having “enough” devices.

**Long-term impact:** Benchmark data becomes misleading when resource locality is absent from provenance.

**Runlith lesson:** capacity evidence should eventually include locality/topology where it materially changes feasibility/cost.

**Existing Runlith protection:** resource ports, hardware-fingerprinted runtime profiles, and provider capacity profiles. Missing explicit topology schema is the remaining gap.

### AP-US-002 — Assume asynchronous overlap is free

**Evidence:** Playbook `ultra_blog.md:2290-2301`.

**Immediate impact:** The scheduler double-counts shared resources; “overlapped” work slows both sides.

**Long-term impact:** routing/placement models learn unstable or hardware-specific conclusions without describing contention.

**Runlith lesson:** concurrency behavior and resource interference need measured evidence, not a Boolean “parallelizable” flag.

**Existing Runlith protection:** `ModelRuntimeProfile` already includes concurrency behavior; the future topology evidence candidate extends this to shared physical-resource contention.

### AP-US-003 — Non-terminal background failure represented as an in-progress state

**Evidence:** Nanotron `src/nanotron/s3_checkpoints/s3_mover.py:194-228`, `373-405`.

**Immediate impact:** callers can block indefinitely after an already-known subprocess failure.

**Long-term impact:** recovery and automation lose deterministic liveness; operators must infer failure from logs.

**Runlith lesson:** explicit terminal failure + bounded retry/wait + escalation are mandatory for background state machines.

**Existing Runlith protection:** bounded planning/retry/liveness invariants and future G11/G12 recovery contracts.

### AP-US-004 — Ambient home-directory credentials as implicit job capability

**Evidence:** Nanotron `src/nanotron/eval/one_job_runner.py:163-171`.

**Immediate impact:** spawned work receives a broader credential than its explicit execution contract states.

**Long-term impact:** authority is difficult to audit/revoke and follows host identity rather than task intent.

**Runlith lesson:** secrets are explicit, scoped, policy-bound capabilities.

**Existing Runlith protection:** Secrets Broker, provider environment isolation, and G12 no-ambient-secret requirement.

### AP-US-005 — Optimize pipeline utilization across semantic authority boundaries

**Evidence:** The Playbook shows that aggressive pipeline/interleaving schedules improve utilization but increase state and scheduling complexity (`ultra_blog.md:1621-1884`). This anti-pattern is a Runlith adoption warning derived from the tradeoff, not a claim that Nanotron violates Runlith semantics.

**Immediate impact in Runlith if copied naively:** speculative execution could outrun required Policy, approval, verification, or recovery decisions.

**Long-term impact:** authority and completion semantics become entangled with throughput scheduling.

**Runlith lesson:** pipeline only work units whose dependency and authority preconditions are already satisfied. Performance scheduling never bypasses durable lifecycle gates.

**Existing Runlith protection:** trusted application services/Reconciler own transitions; Policy controls authority; Verification owns completion evidence.

## Runlith overlap analysis

### External idea: hardware-specific measured runtime profiles

**External evidence:** Playbook `104-128`, `2117-2251`; Nanotron benchmark records `scripts/scaling_benchmarks.py:177-224`.

**Closest Runlith equivalent:** `ModelRuntimeProfile`, `ProviderCapacityProfile`, `BenchmarkEvidencePort`, `ModelProfilerPort`.

**Runlith evidence:** Source of Truth `4999-5014`, `5057-5079`, `5183-5213`; `INV-063`, `INV-080`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none for the generic profiling/provenance concept.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: topology-aware placement

**External evidence:** Playbook `1371-1410`, `2008-2027`; the best parallelism axis changes when traffic crosses physical interconnect boundaries.

**Closest Runlith equivalent:** `DeviceResourceManagerPort`, `ModelResidencyManagerPort`, `ModelRuntimeProfile`, G12 Remote Worker capacity/lifecycle.

**Runlith evidence:** Source of Truth `5197-5213`, G12 `3871-3958`, remote worker requirements `7076`, `7132`; `INV-064`. Repository-wide search found no explicit hardware interconnect/topology/bandwidth model in `crates/`, `adapters/`, or `bins/`.

**Current status:** `NEEDS_ADR`.

**Actual gap:** define how Runlith represents a physical/logical resource locality graph and measured/estimated link/contention evidence without leaking NVIDIA/Apple/cluster-specific types into router domain logic.

**Decision:** `ADAPT`.

### External idea: analytic cost/feasibility preflight before empirical selection

**External evidence:** Playbook Appendix A3 `3777-3908`, selection process `2117-2197`.

**Closest Runlith equivalent:** hard routing eligibility gates, feasibility/cost input from resource manager, versioned RoutePlan, benchmark/profile evidence.

**Runlith evidence:** Source of Truth `5090-5157`, `5197-5213`; `INV-060`, `INV-067`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none justified by this source.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: benchmark/search automation

**External evidence:** Playbook `2199-2283`; Nanotron `scripts/scaling_benchmarks.py`.

**Closest Runlith equivalent:** `ModelProfilerPort`, `BenchmarkEvidencePort`, observe/shadow promotion, measured routing feedback.

**Runlith evidence:** Source of Truth `5183-5195` and existing efficiency/routing architecture; `INV-063`, `INV-080`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** no separate “benchmark thousands of combinations” task is justified. Runlith should benchmark only the configuration dimensions that materially affect its workloads.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: on-demand materialization + prefetch

**External evidence:** Playbook ZeRO-3/FlashAttention sections `794-1012`, `2753-2808`.

**Closest Runlith equivalent:** Context Engine progressive disclosure, Resource Engine exact retrieval, Model Residency, content-addressed evidence/artifacts.

**Runlith evidence:** Context Engine architecture and Source of Truth resource/routing sections.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none from this source.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: resumable versioned checkpoint state independent from one execution layout

**External evidence:** Nanotron `src/nanotron/serialize/main.py:30-43`, metadata and optimizer resharding noted above.

**Closest Runlith equivalent:** provider-independent continuity and durable Task/Execution/Attempt/checkpoint state.

**Runlith evidence:** `INV-019`, `INV-022`, `INV-023`, `INV-066`, `INV-110`; continuity/provider architecture.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** topology-neutral provider-native resume details remain separately tracked by existing research (`IKQ-002`); the Ultra-Scale Playbook adds no new provider-session semantics.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: background transfer state machine

**External evidence:** Nanotron `S3Mover`.

**Closest Runlith equivalent:** G11 background daemon and G12 remote-worker lifecycle/recovery.

**Runlith evidence:** Source of Truth G11/G12 plus liveness/retry invariants.

**Current status:** `SUPERSEDED` as an implementation pattern because the reviewed implementation can fail to reach a terminal state.

**Actual gap:** none. The failure is useful negative evidence for Runlith's existing bounded-liveness requirement.

**Decision:** `REJECT` implementation, retain lesson.

## External validation of existing Runlith design

1. **Separate feasibility from preference scoring.** The Playbook first solves memory/communication feasibility, then optimizes throughput. This validates Runlith's hard eligibility gates before route scoring.
2. **Evidence needs hardware/runtime provenance.** Performance reversals across topology and implementation versions validate `INV-063`/`INV-080`.
3. **Resource/platform details belong behind ports.** The Playbook is highly hardware-dependent; Runlith's `DeviceResourceManagerPort` prevents those details from contaminating provider-neutral routing semantics.
4. **Progressive disclosure/materialization reduces working-set cost.** ZeRO/FlashAttention provide independent systems evidence for Runlith's context/resource materialization direction.
5. **Asynchronous work requires semantics-aware observability.** CUDA timing/profiling validates Runlith's insistence on measured lifecycle evidence rather than naive elapsed-time wrappers.
6. **Global synchronization should be scoped.** Barrier/straggler behavior supports Runlith's minimal-blocking DAG/human-attention design.
7. **Durable state should survive executor topology changes.** Nanotron's topology-agnostic checkpoint ambitions validate Runlith's provider-independent continuity direction.
8. **Background state machines need terminal failure.** `S3Mover` provides concrete negative evidence for Runlith's bounded retry/liveness contracts.
9. **Ambient credentials are the wrong authority model.** Nanotron eval launch scripts reinforce Runlith's Secrets Broker and scoped environment isolation.

## Novel gaps

### NG-US-001 — Provider-neutral hardware topology and contention evidence

Runlith already has device-capacity and runtime-profile abstractions, but the reviewed architecture does not define:

- resource locality groups (same device, same host, same accelerator fabric, same LAN/remote worker boundary);
- measured/estimated link bandwidth/latency evidence with provenance and confidence;
- shared-resource contention/interference observations;
- a topology/version fingerprint used to invalidate stale performance evidence;
- a provider-neutral placement-cost input that Router/Scheduler can consume without importing platform-specific types.

This should remain dormant until a concrete multi-device local-model or G12 remote-worker workload needs it. The Playbook does not justify building a distributed GPU scheduler inside Runlith today.

## Adoption matrix

| Finding | Runlith status | Decision | Meaningful Runlith delta |
|---|---|---|---|
| Hardware/runtime-specific profiles | ALREADY_DESIGNED | EXTERNAL_VALIDATION | Keep provider-neutral provenance and quality/verification dimensions in addition to speed |
| Analytic preflight + empirical benchmark | ALREADY_DESIGNED | EXTERNAL_VALIDATION | Optimize verified delivery rather than raw throughput |
| Physical-topology-aware placement | NEEDS_ADR | ADAPT | Provider-neutral locality/topology evidence; privacy/policy/security remain hard constraints |
| Resource contention evidence | PARTIALLY_EXISTS | ADAPT | Extend concurrency profile into explicit shared-resource interference evidence when needed |
| On-demand materialization/prefetch | ALREADY_DESIGNED | EXTERNAL_VALIDATION | Apply to context/models/artifacts, not GPU tensors specifically |
| Pipeline/microbatch utilization | CONFLICTS_WITH_ARCHITECTURE if applied across authority gates | INSPIRED_ONLY | Pipeline only already-authorized independent execution work |
| Versioned topology-independent checkpoint semantics | ALREADY_DESIGNED | EXTERNAL_VALIDATION | Provider/session neutrality plus Policy/Verification independence |
| S3Mover failure state machine | SUPERSEDED | REJECT | Runlith requires terminal failure/bounded liveness |
| Ambient credential inheritance | CONFLICTS_WITH_ARCHITECTURE | REJECT | Secrets Broker + scoped grants |
| Pickle-backed untrusted persistence | CONFLICTS_WITH_ARCHITECTURE | REJECT | Typed/data-only canonical artifacts and explicit trust boundaries |

## Ideas rejected for Runlith

- **Import DP/TP/PP/CP/EP as Runlith domain concepts.** These are model-training decomposition axes, not engineering-work lifecycle semantics.
- **Optimize for accelerator utilization as a first-class product objective.** Runlith optimizes verified delivery cost under security/privacy/quality constraints; utilization is only one lower-level input.
- **Use global barriers for orchestration simplicity.** Only use synchronization at the smallest correctness boundary.
- **Adopt training-framework checkpoint formats.** Their schema and trust assumptions are unrelated to Runlith's canonical Task/Execution state.
- **Treat shell-generated Slurm jobs or ambient home credentials as reusable execution patterns.** They conflict with the governed Runner and Secrets Broker model.
- **Create a current-G7 implementation task for topology scheduling.** There is no present product requirement that justifies the complexity.

## Candidate ADRs

### ADR-CANDIDATE — Hardware topology and contention evidence contract

**Problem:** `DeviceResourceManagerPort` and G12 worker capacity can report resources, but there is no accepted provider-neutral contract for locality/interconnect/contention evidence that materially changes placement cost.

**Current Runlith state:** resource capacity, model residency, runtime profiles, worker health and provider capacity are already designed; topology detail is absent.

**External inspiration:** Ultra-Scale Playbook's topology-sensitive TP/PP placement, communication/computation overlap analysis, and measured scaling cliffs.

**Actual delta:** define evidence types and ownership, not a GPU scheduler. Candidate concepts: `HardwareTopologyProfile`, `ResourceLocality`, `LinkEvidence`, `ContentionEvidence`, `topology_fingerprint`, confidence/provenance/expiry rules, and a placement-cost projection consumed by Router/Scheduler.

**Architecture impact:** medium, future-facing. Platform-specific probes stay behind resource adapters; Task/Execution identity remains independent from placement.

**Security impact:** topology/capacity data never widens authority. Remote-worker placement still re-evaluates Policy, privacy, scoped secrets, network and isolation requirements.

**Suggested gate:** G12 or the first earlier gate that actually adds multi-device local inference.

**Priority:** LOW now; MEDIUM/HIGH only when the triggering workload exists.

## Candidate implementation tasks

### TASK-CANDIDATE — Topology/contended-resource profiling research spike

**Type:** RESEARCH

**Problem:** determine whether topology/contended-resource evidence materially improves Runlith placement for the actual local-model/remote-worker workload before committing to new domain contracts.

**Current Runlith state:** generic resource and runtime profiles exist in architecture; no implementation of topology-aware placement was found.

**External inspiration:** measured topology cliffs and overlap contention from the Playbook.

**Actual delta:** a bounded prototype that compares capacity-only placement against topology-aware estimates using representative Runlith workloads. No production scheduling semantics until the ADR is accepted.

**Target subsystem:** future Resource Engine / Adaptive Model Router / G12 Worker Scheduler.

**Dependencies:** concrete multi-device or multi-worker use case; accepted measurement methodology; resource-adapter capability to expose evidence safely.

**Architecture impact:** none if retained as research; medium only after ADR adoption.

**Security impact:** measurement probes must remain read-only/bounded and cannot expose host/organization secrets to remote providers.

**Acceptance direction:** demonstrate a repeatable case where topology or contention changes the correct placement decision; otherwise close the candidate with no architecture expansion.

**Verification direction:** fixed workload/hardware fingerprints, repeated trials, warm/cold separation, confidence/error bars where practical, topology-change invalidation, and comparison against capacity-only baseline.

**Suggested gate:** pre-G12 research or G12.

**Priority:** LOW until triggered.

## Source evidence index

Primary source revision for all Playbook entries: `nanotron/ultrascale-playbook@c93890dd4b22356ee18ed9b863e6f2d0909729bc`.

| Finding id | Source | Symbol / section | Region | Confidence |
|---|---|---|---|---|
| US-PAT-001 | `ultra_blog.md` | High level overview; combining axes; finding best config | 104-128; 2008-2027; 2117-2197 | HIGH |
| US-PAT-002 | `ultra_blog.md`; Nanotron `scripts/scaling_benchmarks.py` | benchmark search; overlap math; `create_config`, `save_experiment_configs` | 2117-2251; 3777-3908; Nanotron 35-115, 177-224 | HIGH |
| US-PAT-003 | `ultra_blog.md` | TP/SP; 5D placement | 1371-1410; 2008-2027 | HIGH |
| US-PAT-004 | `ultra_blog.md` | Ring/overlap; benchmark postmortem; Appendix A3 | 1469-1487; 2290-2301; 3777-3908 | HIGH |
| US-PAT-005 | `ultra_blog.md` | ZeRO-3; FlashAttention | 794-1012; 2753-2808 | HIGH |
| US-PAT-006 | Nanotron `src/nanotron/serialize/{main,metadata,optimizer,random}.py` | checkpoint schema and topology-aware reload | cited regions above | HIGH |
| US-PAT-007 | `ultra_blog.md`; Nanotron trainer | Distributed Training Profiling; CUDA timing | 3624-3718; trainer 546-557 | HIGH |
| US-REL-001 | Nanotron `src/nanotron/s3_checkpoints/s3_mover.py` | `wait_for_completion`, `distributed_wait_for_completion`, `update` | 194-228; 373-405 | HIGH |
| US-SEC-001 | Nanotron `src/nanotron/eval/one_job_runner.py` | generated HF token setup | 163-171 | HIGH |
| US-SEC-002 | Nanotron `scripts/weka.py`; `eval/one_job_runner.py`; example launcher | shell construction | weka 105-115; eval 145-283; example 86-95 | MEDIUM |
| US-SEC-003 | Nanotron serialize/data modules | pickle/`torch.load` paths | optimizer 206-211, 332-369; random 10-36; tokenized cache 324-418 | MEDIUM |
| US-SEC-004 | Nanotron `s3_mover.py` | local-path acceptance and cleanup | 70-114; 333-350 | MEDIUM |

Companion implementation revision for Nanotron evidence: `huggingface/nanotron@2411b022a75fb7f7561a1bb4166706da5e1b76de`.

The supplied PDF was visually sampled against the Space source during review. Source-level citations above intentionally bind findings to Git text/code revisions rather than unstable PDF page rendering.
