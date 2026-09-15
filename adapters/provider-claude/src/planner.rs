//! Claude-backed proposal-only `PlannerPort` adapter (TASK-0078, G7).
//!
//! The adapter reuses the existing Claude provider process path. Production
//! composition must attach a verified [`runner::SandboxedLaunch`]; an
//! unattached planner fails closed exactly like `ClaudeExecutor`. The planner
//! parses model output as untrusted data, injects adapter-owned provenance and
//! measured/provider-reported cost evidence, and returns canonical plan-domain
//! values. It has no storage, Task mutation, scheduler, policy, approval, or
//! execution authority.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plan_domain::{
    call_within_envelope, compute_proposal_hash, AtomicityAssessment, AtomicityBlocker,
    AtomicityRequest, AtomicityVerdict, BudgetDelta, CriterionDecl, CriterionId,
    DecompositionRequest, EdgeAddition, EdgeRemoval, EvidenceRef, GateRef, GateUpdate,
    NodeAddition, NodeUpdate, PlanChangeSet, PlanNodeId, PlanScope, PlannerAdapterId, PlannerFault,
    PlannerIdentity, PlannerPort, PlanningBudget, PlanningCostReceipt, ReplanRequest, RiskClass,
    RiskDelta, VerificationMethod, MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_WALL_CLOCK_SECS,
};
use runner::SandboxedLaunch;

use crate::config::{ClaudeConfig, MAX_PROMPT_CHARS};
use crate::executor::ClaudeExecutor;
use crate::json::{parse_json, Json};
use crate::usage::UsageProvenance;

const CLAUDE_PLANNER_ADAPTER_ID: &str = "claude-planner";
const DEFAULT_MODEL_REF: &str = "claude-code/provider-native-default";
const PLANNER_PRICE_CLASS: &str = "provider-reported";

/// Claude implementation of the canonical proposal-only planning seam.
#[derive(Clone)]
pub struct ClaudePlanner {
    executor: ClaudeExecutor,
    identity: PlannerIdentity,
}

impl ClaudePlanner {
    /// Builds an unattached planner. Production must attach a verified Runner
    /// launch with [`Self::with_sandboxed_launch`] before any provider call.
    pub fn bind(
        config: ClaudeConfig,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::bind_with_model(config, workdir, environment_base, DEFAULT_MODEL_REF, false)
    }

    /// Explicit trusted-host constructor for deterministic adapter contract
    /// fixtures. Production composition must use [`Self::bind`].
    pub fn bind_trusted_host(
        config: ClaudeConfig,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::bind_with_model(config, workdir, environment_base, DEFAULT_MODEL_REF, true)
    }

    /// Shared constructor for the explicit production and trusted-host entry
    /// points. Keeping the trust-mode switch private prevents composition code
    /// from accidentally selecting it through a generic boolean option.
    fn bind_with_model(
        config: ClaudeConfig,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
        model_ref: impl Into<String>,
        trusted_host: bool,
    ) -> Result<Self, String> {
        let config = bound_planner_config(config)?;
        let workdir = workdir.into();
        let environment_base = environment_base.into();
        let executor = if trusted_host {
            ClaudeExecutor::bind_trusted_host(
                config,
                "Runlith proposal-only planner",
                workdir,
                environment_base,
            )?
        } else {
            ClaudeExecutor::bind(
                config,
                "Runlith proposal-only planner",
                workdir,
                environment_base,
            )?
        };
        let identity = PlannerIdentity::new(
            PlannerAdapterId::new(CLAUDE_PLANNER_ADAPTER_ID).map_err(|error| error.to_string())?,
            model_ref.into(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self { executor, identity })
    }

    /// Attaches the G6/Runner-verified launch recipe used for every planner
    /// provider call.
    #[must_use]
    pub fn with_sandboxed_launch(mut self, launch: Arc<SandboxedLaunch>) -> Self {
        self.executor = self.executor.with_sandboxed_launch(launch);
        self
    }

    /// Exact executable identity for G6 authorization and Runner planning.
    #[must_use]
    pub fn executable(&self) -> &std::ffi::OsStr {
        self.executor.executable()
    }

    /// Planner work directory bound at construction.
    #[must_use]
    pub fn workdir(&self) -> &Path {
        self.executor.workdir()
    }

    /// Exact explicit provider child environment for Runner planning.
    pub fn child_environment(
        &self,
    ) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
        self.executor.child_environment()
    }

    /// Provider-owned read roots required by the concrete isolation backend.
    pub fn isolation_support_read_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.executor.isolation_support_read_roots()
    }

    /// Provider-owned write roots required by the concrete isolation backend.
    pub fn isolation_support_write_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.executor.isolation_support_write_roots()
    }

    fn call_provider(
        &self,
        prompt: &str,
        budget: Option<PlanningBudget>,
    ) -> Result<(String, PlanningCostReceipt), PlannerFault> {
        if prompt.trim().is_empty() {
            return provider_fault("planner prompt is blank");
        }
        let conservative_input_bound = prompt.len() as u64;
        if prompt.chars().count() > MAX_PROMPT_CHARS
            || conservative_input_bound > u64::from(MAX_PLANNER_INPUT_TOKENS)
        {
            return Err(PlannerFault::BudgetExhausted);
        }
        if let Some(remaining) = budget {
            if remaining.calls_remaining == 0
                || remaining.input_tokens_remaining == 0
                || conservative_input_bound > u64::from(remaining.input_tokens_remaining)
            {
                return Err(PlannerFault::BudgetExhausted);
            }
        }

        let started = Instant::now();
        let outcome =
            self.executor
                .run_prompt(prompt)
                .map_err(|error| PlannerFault::ProviderFailure {
                    typed_cause: error.to_string(),
                })?;
        let wall_time_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if outcome.usage.provenance != UsageProvenance::Reported {
            return provider_fault("claude planner returned no provider-reported token usage");
        }
        let input_tokens = outcome
            .usage
            .total_input_tokens()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| PlannerFault::ProviderFailure {
                typed_cause:
                    "claude planner total input-token usage is missing, overflowed, or out of range"
                        .to_owned(),
            })?;
        let output_tokens = outcome
            .usage
            .output_tokens
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| PlannerFault::ProviderFailure {
                typed_cause: "claude planner output-token usage is missing or out of range"
                    .to_owned(),
            })?;
        let receipt = PlanningCostReceipt::new(
            input_tokens,
            output_tokens,
            wall_time_ms,
            PLANNER_PRICE_CLASS.to_owned(),
        )
        .map_err(|error| PlannerFault::ProviderFailure {
            typed_cause: error.to_string(),
        })?;
        if !call_within_envelope(&receipt) {
            return Err(PlannerFault::BudgetExhausted);
        }
        if let Some(remaining) = budget {
            if input_tokens > remaining.input_tokens_remaining {
                return Err(PlannerFault::BudgetExhausted);
            }
        }
        Ok((outcome.provider_result, receipt))
    }

    fn parse_assessment(
        &self,
        text: &str,
        allowed_evidence: &[EvidenceRef],
    ) -> Result<AtomicityAssessment, PlannerFault> {
        let json = parse_json(text).map_err(malformed_fault)?;
        let verdict = required_str(&json, "verdict")?;
        let rationale = required_str(&json, "rationale")?.to_owned();
        let evidence_refs = match json.get("evidence_refs") {
            Some(value) => parse_evidence_refs(value)?,
            None => allowed_evidence.to_vec(),
        };
        if evidence_refs
            .iter()
            .any(|candidate| !allowed_evidence.iter().any(|allowed| allowed == candidate))
        {
            return provider_fault("atomicity assessment invented an evidence reference");
        }
        let verdict = match verdict {
            "ATOMIC" => AtomicityVerdict::Atomic,
            "DECOMPOSABLE" => AtomicityVerdict::Decomposable,
            "BLOCKED" => {
                let blocker = required(&json, "blocker")?;
                AtomicityVerdict::Blocked {
                    blocker: AtomicityBlocker::new(
                        required_str(blocker, "code")?.to_owned(),
                        required_str(blocker, "detail")?.to_owned(),
                    )
                    .map_err(domain_fault)?,
                }
            }
            other => return provider_fault(format!("unknown atomicity verdict {other:?}")),
        };
        AtomicityAssessment::new(verdict, rationale, evidence_refs).map_err(domain_fault)
    }

    fn parse_proposal(
        &self,
        text: &str,
        receipt: PlanningCostReceipt,
        source_evidence_refs: Vec<EvidenceRef>,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let json = parse_json(text).map_err(malformed_fault)?;
        let mut proposal = PlanChangeSet {
            base_plan_revision: required_u64(&json, "base_plan_revision")?,
            reason: required_str(&json, "reason")?.to_owned(),
            source_evidence_refs,
            node_additions: parse_node_additions(required(&json, "node_additions")?)?,
            node_updates: parse_node_updates(required(&json, "node_updates")?)?,
            node_removals: parse_node_ids(required(&json, "node_removals")?)?,
            edge_additions: parse_edge_additions(required(&json, "edge_additions")?)?,
            edge_removals: parse_edge_removals(required(&json, "edge_removals")?)?,
            expected_outcome: required_str(&json, "expected_outcome")?.to_owned(),
            risk_delta: parse_risk_delta(required(&json, "risk_delta")?)?,
            budget_delta: parse_budget_delta(required(&json, "budget_delta")?)?,
            scope_delta: parse_scope(required(&json, "scope_delta")?)?,
            coverage_map: parse_coverage_map(required(&json, "coverage_map")?)?,
            proposal_hash: String::new(),
            proposed_by: self.identity.clone(),
            planning_cost_receipt: receipt,
        };
        proposal.proposal_hash = compute_proposal_hash(&proposal);
        Ok(proposal)
    }
}

impl PlannerPort for ClaudePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        let prompt = atomicity_prompt(&input);
        let (text, _receipt) = self.call_provider(&prompt, None)?;
        self.parse_assessment(&text, &input.evidence_refs)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        if input.depth_remaining == 0 {
            return Err(PlannerFault::DepthExhausted);
        }
        if input.budget_remaining.calls_remaining == 0
            || input.budget_remaining.input_tokens_remaining == 0
        {
            return Err(PlannerFault::BudgetExhausted);
        }
        let prompt = decomposition_prompt(&input);
        let (text, receipt) = self.call_provider(&prompt, Some(input.budget_remaining))?;
        self.parse_proposal(&text, receipt, input.evidence_refs)
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        if input.depth_remaining == 0 {
            return Err(PlannerFault::DepthExhausted);
        }
        if input.budget_remaining.calls_remaining == 0
            || input.budget_remaining.input_tokens_remaining == 0
        {
            return Err(PlannerFault::BudgetExhausted);
        }
        let prompt = replan_prompt(&input);
        let (text, receipt) = self.call_provider(&prompt, Some(input.budget_remaining))?;
        self.parse_proposal(&text, receipt, input.triggering_evidence_refs)
    }
}

fn bound_planner_config(config: ClaudeConfig) -> Result<ClaudeConfig, String> {
    let maximum = Duration::from_secs(MAX_PLANNER_WALL_CLOCK_SECS);
    let config = if config.timeout() > maximum {
        config.with_timeout(maximum)?
    } else {
        config
    };
    // The planner is proposal-only. Provider-native plan mode is defense in
    // depth on top of the mandatory Runner sandbox; callers cannot widen it
    // by passing an execution-capable permission mode in ClaudeConfig.
    config.with_permission_mode("plan")
}

fn provider_fault<T>(detail: impl Into<String>) -> Result<T, PlannerFault> {
    Err(PlannerFault::ProviderFailure {
        typed_cause: detail.into(),
    })
}

fn domain_fault(error: plan_domain::PlanDomainError) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: format!("malformed planner output: {error}"),
    }
}

fn malformed_fault(error: impl std::fmt::Display) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: format!("malformed planner output: {error}"),
    }
}

fn required<'a>(json: &'a Json, key: &str) -> Result<&'a Json, PlannerFault> {
    json.get(key).ok_or_else(|| PlannerFault::ProviderFailure {
        typed_cause: format!("malformed planner output: missing {key:?}"),
    })
}

fn required_str<'a>(json: &'a Json, key: &str) -> Result<&'a str, PlannerFault> {
    required(json, key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause: format!("malformed planner output: {key:?} must be a non-blank string"),
        })
}

fn required_u64(json: &Json, key: &str) -> Result<u64, PlannerFault> {
    required(json, key)?
        .as_u64()
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause: format!("malformed planner output: {key:?} must be an unsigned integer"),
        })
}

fn array(value: &Json) -> Result<&[Json], PlannerFault> {
    value
        .as_array()
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause: "malformed planner output: expected array".to_owned(),
        })
}

fn parse_strings(value: &Json) -> Result<Vec<String>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| PlannerFault::ProviderFailure {
                    typed_cause: "malformed planner output: array entry must be non-blank string"
                        .to_owned(),
                })
        })
        .collect()
}

fn parse_evidence_refs(value: &Json) -> Result<Vec<EvidenceRef>, PlannerFault> {
    parse_strings(value)?
        .into_iter()
        .map(|value| EvidenceRef::new(value).map_err(domain_fault))
        .collect()
}

fn parse_scope(value: &Json) -> Result<PlanScope, PlannerFault> {
    PlanScope::new(parse_strings(value)?).map_err(domain_fault)
}

fn parse_node_ids(value: &Json) -> Result<Vec<PlanNodeId>, PlannerFault> {
    parse_strings(value)?
        .into_iter()
        .map(|value| PlanNodeId::new(value).map_err(domain_fault))
        .collect()
}

fn parse_risk(value: &str) -> Result<RiskClass, PlannerFault> {
    match value {
        "low" => Ok(RiskClass::Low),
        "medium" => Ok(RiskClass::Medium),
        "high" => Ok(RiskClass::High),
        "critical" => Ok(RiskClass::Critical),
        other => provider_fault(format!(
            "malformed planner output: unknown risk class {other:?}"
        )),
    }
}

fn parse_method(value: &str) -> Result<VerificationMethod, PlannerFault> {
    VerificationMethod::from_str(value).map_err(domain_fault)
}

fn parse_criteria(value: &Json) -> Result<Vec<CriterionDecl>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(CriterionDecl::new(
                CriterionId::new(required_str(item, "id")?.to_owned()).map_err(domain_fault)?,
                parse_method(required_str(item, "method")?)?,
            ))
        })
        .collect()
}

fn parse_optional_gate(value: Option<&Json>) -> Result<Option<GateRef>, PlannerFault> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .ok_or_else(|| PlannerFault::ProviderFailure {
                typed_cause: "malformed planner output: required_gate must be string or null"
                    .to_owned(),
            })
            .and_then(|gate| GateRef::new(gate.to_owned()).map_err(domain_fault))
            .map(Some),
    }
}

fn parse_node_additions(value: &Json) -> Result<Vec<NodeAddition>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(NodeAddition {
                id: PlanNodeId::new(required_str(item, "id")?.to_owned()).map_err(domain_fault)?,
                scope: parse_scope(required(item, "scope")?)?,
                criteria: parse_criteria(required(item, "criteria")?)?,
                risk_class: parse_risk(required_str(item, "risk_class")?)?,
                required_gate: parse_optional_gate(item.get("required_gate"))?,
            })
        })
        .collect()
}

fn parse_gate_update(value: Option<&Json>) -> Result<Option<GateUpdate>, PlannerFault> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Obj(_)) => {
            let value = value.expect("matched Some");
            if value.get("clear").and_then(Json::as_bool) == Some(true) {
                return Ok(Some(GateUpdate::Clear));
            }
            let gate = required_str(value, "set")?;
            Ok(Some(GateUpdate::Set(
                GateRef::new(gate.to_owned()).map_err(domain_fault)?,
            )))
        }
        Some(_) => provider_fault(
            "malformed planner output: node update required_gate must be null or object",
        ),
    }
}

fn parse_node_updates(value: &Json) -> Result<Vec<NodeUpdate>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            let scope = match item.get("scope") {
                None | Some(Json::Null) => None,
                Some(scope) => Some(parse_scope(scope)?),
            };
            let risk_class = match item.get("risk_class") {
                None | Some(Json::Null) => None,
                Some(risk) => Some(parse_risk(risk.as_str().ok_or_else(|| {
                    PlannerFault::ProviderFailure {
                        typed_cause:
                            "malformed planner output: node update risk_class must be string"
                                .to_owned(),
                    }
                })?)?),
            };
            Ok(NodeUpdate {
                target: PlanNodeId::new(required_str(item, "target")?.to_owned())
                    .map_err(domain_fault)?,
                scope,
                risk_class,
                required_gate: parse_gate_update(item.get("required_gate"))?,
            })
        })
        .collect()
}

fn parse_edge_additions(value: &Json) -> Result<Vec<EdgeAddition>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(EdgeAddition {
                from: PlanNodeId::new(required_str(item, "from")?.to_owned())
                    .map_err(domain_fault)?,
                to: PlanNodeId::new(required_str(item, "to")?.to_owned()).map_err(domain_fault)?,
            })
        })
        .collect()
}

fn parse_edge_removals(value: &Json) -> Result<Vec<EdgeRemoval>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(EdgeRemoval {
                from: PlanNodeId::new(required_str(item, "from")?.to_owned())
                    .map_err(domain_fault)?,
                to: PlanNodeId::new(required_str(item, "to")?.to_owned()).map_err(domain_fault)?,
            })
        })
        .collect()
}

fn parse_risk_delta(value: &Json) -> Result<Vec<RiskDelta>, PlannerFault> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(RiskDelta {
                node: PlanNodeId::new(required_str(item, "node")?.to_owned())
                    .map_err(domain_fault)?,
                from: parse_risk(required_str(item, "from")?)?,
                to: parse_risk(required_str(item, "to")?)?,
            })
        })
        .collect()
}

fn parse_budget_delta(value: &Json) -> Result<BudgetDelta, PlannerFault> {
    let planning = required(value, "planning_cost_movement")?
        .as_i64()
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause:
                "malformed planner output: planning_cost_movement must be a signed integer"
                    .to_owned(),
        })?;
    let execution = required(value, "expected_execution_cost_movement")?
        .as_i64()
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause:
                "malformed planner output: expected_execution_cost_movement must be a signed integer"
                    .to_owned(),
        })?;
    Ok(BudgetDelta::new(planning, execution))
}

fn parse_coverage_map(
    value: &Json,
) -> Result<BTreeMap<CriterionId, Vec<CriterionId>>, PlannerFault> {
    let object = value
        .as_object()
        .ok_or_else(|| PlannerFault::ProviderFailure {
            typed_cause: "malformed planner output: coverage_map must be an object".to_owned(),
        })?;
    object
        .iter()
        .map(|(parent, children)| {
            let parent = CriterionId::new(parent.clone()).map_err(domain_fault)?;
            let children = parse_strings(children)?
                .into_iter()
                .map(|child| CriterionId::new(child).map_err(domain_fault))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((parent, children))
        })
        .collect()
}

fn atomicity_prompt(input: &AtomicityRequest) -> String {
    format!(
        concat!(
            "You are Runlith's proposal-only planner. Return JSON only; no markdown. ",
            "You have no mutation, execution, policy, approval, or completion authority.\n",
            "Request: {{\"kind\":\"assess_atomicity\",\"task_ref\":{},",
            "\"acceptance_criteria_refs\":{},\"scope_bound\":{},\"evidence_refs\":{},",
            "\"execution_policy_ref\":{}}}\n",
            "Response schema: {{\"verdict\":\"ATOMIC|DECOMPOSABLE|BLOCKED\",",
            "\"rationale\":\"non-empty\",\"evidence_refs\":[\"existing input refs only\"],",
            "\"blocker\":{{\"code\":\"...\",\"detail\":\"...\"}}}}. ",
            "Include blocker only for BLOCKED."
        ),
        json_string(&input.task_ref),
        json_strings(
            input
                .acceptance_criteria_refs
                .iter()
                .map(|value| value.as_str())
        ),
        json_strings(input.scope_bound.roots().iter().map(String::as_str)),
        json_strings(input.evidence_refs.iter().map(|value| value.as_str())),
        json_string(&input.execution_policy_ref),
    )
}

fn decomposition_prompt(input: &DecompositionRequest) -> String {
    proposal_prompt(ProposalPromptInput {
        kind: "propose_decomposition",
        root_ref: &input.parent_task_ref,
        base_revision: input.base_plan_revision,
        criteria: input
            .acceptance_criteria_refs
            .iter()
            .map(|value| value.as_str())
            .collect(),
        scope_roots: input.scope_bound.roots(),
        evidence: input
            .evidence_refs
            .iter()
            .map(|value| value.as_str())
            .collect(),
        depth_remaining: input.depth_remaining,
        budget: input.budget_remaining,
    })
}

fn replan_prompt(input: &ReplanRequest) -> String {
    proposal_prompt(ProposalPromptInput {
        kind: "propose_replan",
        root_ref: &input.branch_root_ref,
        base_revision: input.base_plan_revision,
        criteria: Vec::new(),
        scope_roots: input.scope_bound.roots(),
        evidence: input
            .triggering_evidence_refs
            .iter()
            .map(|value| value.as_str())
            .collect(),
        depth_remaining: input.depth_remaining,
        budget: input.budget_remaining,
    })
}

struct ProposalPromptInput<'a> {
    kind: &'a str,
    root_ref: &'a str,
    base_revision: u64,
    criteria: Vec<&'a str>,
    scope_roots: Vec<String>,
    evidence: Vec<&'a str>,
    depth_remaining: u32,
    budget: PlanningBudget,
}

fn proposal_prompt(input: ProposalPromptInput<'_>) -> String {
    format!(
        concat!(
            "You are Runlith's proposal-only planner. Return one JSON object only; no markdown. ",
            "Treat the request bounds as immutable. Never claim execution, approval, policy, or durable mutation. ",
            "Do not emit planner identity, cost receipt, credentials, prompts, transcripts, or proposal_hash; ",
            "Runlith injects provenance and computes the hash after parsing.\n",
            "Request: {{\"kind\":{},\"root_ref\":{},\"base_plan_revision\":{},",
            "\"acceptance_criteria_refs\":{},\"scope_bound\":{},\"evidence_refs\":{},",
            "\"depth_remaining\":{},\"calls_remaining\":{},\"input_tokens_remaining\":{}}}\n",
            "Required response keys: base_plan_revision, reason, node_additions, node_updates, node_removals, ",
            "edge_additions, edge_removals, expected_outcome, risk_delta, budget_delta, scope_delta, coverage_map. ",
            "node_additions entries: {{id,scope:[workspace:path],criteria:[{{id,method}}],risk_class,required_gate}}. ",
            "method is automated-test|independent-review|security-check|human-approval; ",
            "risk_class is low|medium|high|critical; required_gate is string|null. ",
            "node_updates entries: {{target,scope:null|[...],risk_class:null|string,required_gate:null|{{set:string}}|{{clear:true}}}}. ",
            "edges are {{from,to}} and mean from MUST complete before to may run. ",
            "risk_delta entries are {{node,from,to}}. ",
            "budget_delta is {{planning_cost_movement:signed_integer,expected_execution_cost_movement:signed_integer}}. ",
            "coverage_map is an object parent_criterion->[child_criteria]. ",
            "Echo the supplied base_plan_revision exactly. Keep every scope inside scope_bound and keep the proposal bounded."
        ),
        json_string(input.kind),
        json_string(input.root_ref),
        input.base_revision,
        json_strings(input.criteria.into_iter()),
        json_strings(input.scope_roots.iter().map(String::as_str)),
        json_strings(input.evidence.into_iter()),
        input.depth_remaining,
        input.budget.calls_remaining,
        input.budget.input_tokens_remaining,
    )
}

fn json_strings<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let values = values.map(json_string).collect::<Vec<_>>().join(",");
    format!("[{values}]")
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_timeout_is_clamped_to_g7_bound() {
        let config = ClaudeConfig::default();
        let bounded = bound_planner_config(config).expect("bounded config");
        assert_eq!(
            bounded.timeout(),
            Duration::from_secs(MAX_PLANNER_WALL_CLOCK_SECS)
        );
        assert_eq!(bounded.permission_mode(), Some("plan"));
    }

    #[test]
    fn planner_forces_proposal_only_provider_permission_mode() {
        let config = ClaudeConfig::default()
            .with_permission_mode("acceptEdits")
            .expect("ordinary executor mode remains representable");
        let bounded = bound_planner_config(config).expect("planner config");
        assert_eq!(bounded.permission_mode(), Some("plan"));
    }

    #[test]
    fn json_string_escapes_provider_input_without_shell_semantics() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
}
