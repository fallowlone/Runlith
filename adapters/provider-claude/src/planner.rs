//! Claude-backed proposal-only `PlannerPort` adapter (TASK-0078, G7).
//!
//! The adapter reuses the existing Claude provider process path. Production
//! composition must attach a verified [`runner::SandboxedLaunch`]; an
//! unattached planner fails closed exactly like `ClaudeExecutor`. The planner
//! parses model output as untrusted data, injects adapter-owned provenance and
//! measured/provider-reported cost evidence, and returns canonical plan-domain
//! values. It has no storage, Task mutation, scheduler, policy, approval, or
//! execution authority.
//!
//! Every fault reports [`PlannerCallSpend`]: refusals before the provider
//! child exists are `NotStarted`; once a child may have run, provider-reported
//! usage is preserved as a measured receipt (even when the call then breaches
//! the envelope or returns unusable output), otherwise the spend is
//! `Unmeasured` so the trusted service charges conservatively.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plan_domain::{
    call_within_envelope, compute_proposal_hash, AtomicityAssessment, AtomicityBlocker,
    AtomicityRequest, AtomicityVerdict, BudgetDelta, CriterionDecl, CriterionId,
    DecompositionRequest, EdgeAddition, EdgeRemoval, EvidenceRef, GateRef, GateUpdate,
    NodeAddition, NodeUpdate, PlanChangeSet, PlanNodeId, PlanScope, PlannerAdapterId,
    PlannerCallSpend, PlannerFault, PlannerIdentity, PlannerPort, PlanningBudget,
    PlanningCostReceipt, ReplanRequest, RiskClass, RiskDelta, VerificationMethod,
    MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_WALL_CLOCK_SECS,
};
use runner::SandboxedLaunch;

use crate::config::{ClaudeConfig, MAX_PROMPT_CHARS};
use crate::executor::{ClaudeExecutor, ObservedPromptRun};
use crate::json::{parse_json, Json};
use crate::usage::{UsageProvenance, UsageSample};

const CLAUDE_PLANNER_ADAPTER_ID: &str = "claude-planner";
const DEFAULT_MODEL_REF: &str = "claude-code/provider-native-default";
const PLANNER_PRICE_CLASS: &str = "provider-reported";

/// Claude implementation of the canonical proposal-only planning seam.
#[derive(Clone)]
pub struct ClaudePlanner {
    executor: ClaudeExecutor,
    identity: PlannerIdentity,
}

/// Malformed planner output detail. Converted into a typed
/// `ProviderFailure` that carries the measured spend of the call.
struct Malformed(String);

type Parsed<T> = Result<T, Malformed>;

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

    /// Runs one bounded provider call. `Ok` always carries the measured
    /// receipt; every `Err` carries the spend observed for this call.
    fn call_provider(
        &self,
        prompt: &str,
        budget: Option<PlanningBudget>,
    ) -> Result<(String, PlanningCostReceipt), PlannerFault> {
        if prompt.trim().is_empty() {
            return Err(provider_failure(
                "planner prompt is blank",
                PlannerCallSpend::NotStarted,
            ));
        }
        let conservative_input_bound = prompt.len() as u64;
        if prompt.chars().count() > MAX_PROMPT_CHARS
            || conservative_input_bound > u64::from(MAX_PLANNER_INPUT_TOKENS)
        {
            return Err(budget_exhausted(PlannerCallSpend::NotStarted));
        }
        if let Some(remaining) = budget {
            if remaining.calls_remaining == 0
                || remaining.input_tokens_remaining == 0
                || conservative_input_bound > u64::from(remaining.input_tokens_remaining)
            {
                return Err(budget_exhausted(PlannerCallSpend::NotStarted));
            }
        }

        let started = Instant::now();
        let (result, usage) = match self.executor.run_prompt_observed(prompt) {
            ObservedPromptRun::NotStarted(failure) => {
                return Err(provider_failure(
                    failure.to_string(),
                    PlannerCallSpend::NotStarted,
                ));
            }
            ObservedPromptRun::Started { result, usage } => (result, usage),
        };
        let wall_time_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let measured = measured_receipt(&usage, wall_time_ms);
        let spend = match &measured {
            Ok(receipt) => PlannerCallSpend::Measured(receipt.clone()),
            Err(_) => PlannerCallSpend::Unmeasured,
        };
        let outcome = result.map_err(|failure| provider_failure(failure.to_string(), spend))?;
        let receipt =
            measured.map_err(|detail| provider_failure(detail, PlannerCallSpend::Unmeasured))?;
        if !call_within_envelope(&receipt) {
            return Err(budget_exhausted(PlannerCallSpend::Measured(receipt)));
        }
        if let Some(remaining) = budget {
            if receipt.input_tokens > remaining.input_tokens_remaining {
                return Err(budget_exhausted(PlannerCallSpend::Measured(receipt)));
            }
        }
        Ok((outcome.provider_result, receipt))
    }

    fn parse_assessment(
        &self,
        text: &str,
        allowed_evidence: &[EvidenceRef],
        receipt: PlanningCostReceipt,
    ) -> Parsed<AtomicityAssessment> {
        let json = parse_json(text).map_err(malformed)?;
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
            return Err(Malformed(
                "atomicity assessment invented an evidence reference".to_owned(),
            ));
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
                    .map_err(malformed)?,
                }
            }
            other => return Err(Malformed(format!("unknown atomicity verdict {other:?}"))),
        };
        AtomicityAssessment::new(verdict, rationale, evidence_refs, receipt).map_err(malformed)
    }

    fn parse_proposal(
        &self,
        text: &str,
        receipt: PlanningCostReceipt,
        source_evidence_refs: Vec<EvidenceRef>,
    ) -> Parsed<PlanChangeSet> {
        let json = parse_json(text).map_err(malformed)?;
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
        let (text, receipt) = self.call_provider(&prompt, None)?;
        self.parse_assessment(&text, &input.evidence_refs, receipt.clone())
            .map_err(|detail| malformed_after_spend(detail, receipt))
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
            return Err(budget_exhausted(PlannerCallSpend::NotStarted));
        }
        let prompt = decomposition_prompt(&input);
        let (text, receipt) = self.call_provider(&prompt, Some(input.budget_remaining))?;
        self.parse_proposal(&text, receipt.clone(), input.evidence_refs)
            .map_err(|detail| malformed_after_spend(detail, receipt))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        if input.depth_remaining == 0 {
            return Err(PlannerFault::DepthExhausted);
        }
        if input.budget_remaining.calls_remaining == 0
            || input.budget_remaining.input_tokens_remaining == 0
        {
            return Err(budget_exhausted(PlannerCallSpend::NotStarted));
        }
        let prompt = replan_prompt(&input);
        let (text, receipt) = self.call_provider(&prompt, Some(input.budget_remaining))?;
        self.parse_proposal(&text, receipt.clone(), input.triggering_evidence_refs)
            .map_err(|detail| malformed_after_spend(detail, receipt))
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

/// Builds the exact receipt from provider-reported usage. Missing,
/// overflowing, or out-of-range values are never estimated.
fn measured_receipt(usage: &UsageSample, wall_time_ms: u64) -> Result<PlanningCostReceipt, String> {
    if usage.provenance != UsageProvenance::Reported {
        return Err("claude planner returned no provider-reported token usage".to_owned());
    }
    let input_tokens = usage
        .total_input_tokens()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            "claude planner total input-token usage is missing, overflowed, or out of range"
                .to_owned()
        })?;
    let output_tokens = usage
        .output_tokens
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "claude planner output-token usage is missing or out of range".to_owned())?;
    PlanningCostReceipt::new(
        input_tokens,
        output_tokens,
        wall_time_ms,
        PLANNER_PRICE_CLASS.to_owned(),
    )
    .map_err(|error| error.to_string())
}

fn provider_failure(detail: impl Into<String>, spend: PlannerCallSpend) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: detail.into(),
        spend,
    }
}

fn budget_exhausted(spend: PlannerCallSpend) -> PlannerFault {
    PlannerFault::BudgetExhausted { spend }
}

fn malformed_after_spend(detail: Malformed, receipt: PlanningCostReceipt) -> PlannerFault {
    provider_failure(
        format!("malformed planner output: {}", detail.0),
        PlannerCallSpend::Measured(receipt),
    )
}

fn malformed(error: impl std::fmt::Display) -> Malformed {
    Malformed(error.to_string())
}

fn required<'a>(json: &'a Json, key: &str) -> Parsed<&'a Json> {
    json.get(key)
        .ok_or_else(|| Malformed(format!("missing {key:?}")))
}

fn required_str<'a>(json: &'a Json, key: &str) -> Parsed<&'a str> {
    required(json, key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Malformed(format!("{key:?} must be a non-blank string")))
}

fn required_u64(json: &Json, key: &str) -> Parsed<u64> {
    required(json, key)?
        .as_u64()
        .ok_or_else(|| Malformed(format!("{key:?} must be an unsigned integer")))
}

fn array(value: &Json) -> Parsed<&[Json]> {
    value
        .as_array()
        .ok_or_else(|| Malformed("expected array".to_owned()))
}

fn parse_strings(value: &Json) -> Parsed<Vec<String>> {
    array(value)?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| Malformed("array entry must be non-blank string".to_owned()))
        })
        .collect()
}

fn parse_evidence_refs(value: &Json) -> Parsed<Vec<EvidenceRef>> {
    parse_strings(value)?
        .into_iter()
        .map(|value| EvidenceRef::new(value).map_err(malformed))
        .collect()
}

fn parse_scope(value: &Json) -> Parsed<PlanScope> {
    PlanScope::new(parse_strings(value)?).map_err(malformed)
}

fn parse_node_ids(value: &Json) -> Parsed<Vec<PlanNodeId>> {
    parse_strings(value)?
        .into_iter()
        .map(|value| PlanNodeId::new(value).map_err(malformed))
        .collect()
}

fn parse_risk(value: &str) -> Parsed<RiskClass> {
    match value {
        "low" => Ok(RiskClass::Low),
        "medium" => Ok(RiskClass::Medium),
        "high" => Ok(RiskClass::High),
        "critical" => Ok(RiskClass::Critical),
        other => Err(Malformed(format!("unknown risk class {other:?}"))),
    }
}

fn parse_method(value: &str) -> Parsed<VerificationMethod> {
    VerificationMethod::from_str(value).map_err(malformed)
}

fn parse_criteria(value: &Json) -> Parsed<Vec<CriterionDecl>> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(CriterionDecl::new(
                CriterionId::new(required_str(item, "id")?.to_owned()).map_err(malformed)?,
                parse_method(required_str(item, "method")?)?,
            ))
        })
        .collect()
}

fn parse_optional_gate(value: Option<&Json>) -> Parsed<Option<GateRef>> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .ok_or_else(|| Malformed("required_gate must be string or null".to_owned()))
            .and_then(|gate| GateRef::new(gate.to_owned()).map_err(malformed))
            .map(Some),
    }
}

fn parse_node_additions(value: &Json) -> Parsed<Vec<NodeAddition>> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(NodeAddition {
                id: PlanNodeId::new(required_str(item, "id")?.to_owned()).map_err(malformed)?,
                scope: parse_scope(required(item, "scope")?)?,
                criteria: parse_criteria(required(item, "criteria")?)?,
                risk_class: parse_risk(required_str(item, "risk_class")?)?,
                required_gate: parse_optional_gate(item.get("required_gate"))?,
            })
        })
        .collect()
}

fn parse_gate_update(value: Option<&Json>) -> Parsed<Option<GateUpdate>> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(object @ Json::Obj(_)) => {
            if object.get("clear").and_then(Json::as_bool) == Some(true) {
                return Ok(Some(GateUpdate::Clear));
            }
            let gate = required_str(object, "set")?;
            Ok(Some(GateUpdate::Set(
                GateRef::new(gate.to_owned()).map_err(malformed)?,
            )))
        }
        Some(_) => Err(Malformed(
            "node update required_gate must be null or object".to_owned(),
        )),
    }
}

fn parse_node_updates(value: &Json) -> Parsed<Vec<NodeUpdate>> {
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
                    Malformed("node update risk_class must be string".to_owned())
                })?)?),
            };
            Ok(NodeUpdate {
                target: PlanNodeId::new(required_str(item, "target")?.to_owned())
                    .map_err(malformed)?,
                scope,
                risk_class,
                required_gate: parse_gate_update(item.get("required_gate"))?,
            })
        })
        .collect()
}

fn parse_edge_additions(value: &Json) -> Parsed<Vec<EdgeAddition>> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(EdgeAddition {
                from: PlanNodeId::new(required_str(item, "from")?.to_owned()).map_err(malformed)?,
                to: PlanNodeId::new(required_str(item, "to")?.to_owned()).map_err(malformed)?,
            })
        })
        .collect()
}

fn parse_edge_removals(value: &Json) -> Parsed<Vec<EdgeRemoval>> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(EdgeRemoval {
                from: PlanNodeId::new(required_str(item, "from")?.to_owned()).map_err(malformed)?,
                to: PlanNodeId::new(required_str(item, "to")?.to_owned()).map_err(malformed)?,
            })
        })
        .collect()
}

fn parse_risk_delta(value: &Json) -> Parsed<Vec<RiskDelta>> {
    array(value)?
        .iter()
        .map(|item| {
            Ok(RiskDelta {
                node: PlanNodeId::new(required_str(item, "node")?.to_owned()).map_err(malformed)?,
                from: parse_risk(required_str(item, "from")?)?,
                to: parse_risk(required_str(item, "to")?)?,
            })
        })
        .collect()
}

fn parse_budget_delta(value: &Json) -> Parsed<BudgetDelta> {
    let planning = required(value, "planning_cost_movement")?
        .as_i64()
        .ok_or_else(|| Malformed("planning_cost_movement must be a signed integer".to_owned()))?;
    let execution = required(value, "expected_execution_cost_movement")?
        .as_i64()
        .ok_or_else(|| {
            Malformed("expected_execution_cost_movement must be a signed integer".to_owned())
        })?;
    Ok(BudgetDelta::new(planning, execution))
}

fn parse_coverage_map(value: &Json) -> Parsed<BTreeMap<CriterionId, Vec<CriterionId>>> {
    let object = value
        .as_object()
        .ok_or_else(|| Malformed("coverage_map must be an object".to_owned()))?;
    object
        .iter()
        .map(|(parent, children)| {
            let parent = CriterionId::new(parent.clone()).map_err(malformed)?;
            let children = parse_strings(children)?
                .into_iter()
                .map(|child| CriterionId::new(child).map_err(malformed))
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
            // G7 v1 materializes automated-test obligations only; a child declaring
            // another method is refused at materialization (ADR-0027 TASK-0096),
            // so the prompt must not offer what the system will reject.
            "method is automated-test; ",
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

    #[test]
    fn measured_receipt_never_estimates_partial_usage() {
        let full = UsageSample::reported(Some(5), Some(40_000), None, Some(9));
        let receipt = measured_receipt(&full, 7).expect("exact receipt");
        assert_eq!((receipt.input_tokens, receipt.output_tokens), (40_005, 9));
        for partial in [
            UsageSample::unknown(),
            UsageSample::reported(None, Some(10), None, Some(9)),
            UsageSample::reported(Some(10), None, None, None),
            UsageSample::reported(Some(u64::from(u32::MAX) + 1), None, None, Some(1)),
        ] {
            assert!(measured_receipt(&partial, 7).is_err());
        }
    }
}
