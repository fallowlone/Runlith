use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use plan_domain::{PlanningCostReceipt, PlanningCostReceiptRef};
use storage_sqlite::{SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-plan-cost-receipts-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn receipt(input: u32) -> PlanningCostReceipt {
    PlanningCostReceipt::new(input, 40, 850, "standard".to_owned()).expect("receipt")
}

#[test]
fn plan_cost_receipt_round_trips_and_identity_is_immutable() {
    let db = TestDb::new("round-trip");
    let mut store = SqliteStore::open(&db.path).expect("store");
    let receipt_ref = PlanningCostReceiptRef::new("planning-receipt:test-1").expect("ref");
    let evidence = receipt(120);

    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("record receipt");
    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("exact replay is idempotent");
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).expect("read"),
        Some(evidence)
    );

    let error = store
        .record_plan_cost_receipt(&receipt_ref, &receipt(121))
        .expect_err("receipt identity cannot be rebound");
    assert!(matches!(error, StorageError::ConstraintViolation(_)));
}

#[test]
fn schema_12_database_upgrades_to_current_without_plan_cost_data() {
    let db = TestDb::new("upgrade-12-current");
    {
        let store = SqliteStore::open(&db.path).expect("fresh current store");
        assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    }
    {
        let connection = rusqlite::Connection::open(&db.path).expect("raw sqlite");
        connection
            .execute_batch(
                "DROP TRIGGER IF EXISTS event_journal_no_update;
                 ALTER TABLE event_journal DROP COLUMN previous_hash;
                 ALTER TABLE event_journal DROP COLUMN record_hash;
                 DROP TRIGGER IF EXISTS event_journal_no_delete;
                 DROP TRIGGER IF EXISTS event_plan_causes_no_update;
                 DROP TRIGGER IF EXISTS event_plan_causes_no_delete;
                 DROP TABLE task_prerequisites;
                 DROP TABLE evaluated_policy_bases;
                 DROP TABLE test_mutation_policy_revisions;
                 DROP TABLE task_evidence_requirement_params;
                 DROP TABLE task_evidence_requirements;
                 DROP TABLE candidate_input_assessments;
                 DROP TABLE candidate_input_operations;
                 DROP TABLE observed_effects;
                 DROP TABLE agent_claims;
                 DROP TABLE claim_verifications;
                 DROP TABLE policy_exemptions;
                 DROP TABLE approval_discharges;
                 DROP TABLE completion_admissions;
                 DROP TABLE planning_input_candidate_tasks;
                 DROP TABLE human_decision_evidence;
                 DROP TABLE human_decision_options;
                 DROP TABLE human_decision_scope_roots;
                 DROP TABLE human_decision_requests;
                 DROP TABLE planning_run_heads;
                 DROP TABLE planning_runs;
                 DROP TABLE planning_input_heads;
                 DROP TABLE planning_input_lineage;
                 DROP TABLE planning_input_evidence;
                 DROP TABLE planning_input_criterion_requirement_params;
                 DROP TABLE planning_input_criteria;
                 DROP TABLE planning_input_scope_roots;
                 DROP TABLE planning_input_snapshots;
                 DROP TABLE plan_guardrail_events;
                 DROP TABLE plan_revision_lineage;
                 DROP TABLE plan_cost_receipts;
                 DROP TABLE review_records;
                 DROP TABLE review_path_settings;
                 DROP TABLE reviewer_profiles;
                 DROP TABLE owner_presence_proofs;
                 DROP TABLE binding_proof_consumption;
                 DROP TABLE side_effect_escalations;
                 DROP TABLE side_effect_transitions;
                 DROP TABLE side_effect_intents;
                 DROP TABLE context_trace_revs;
                 DROP TABLE context_trace_decisions;
                 DROP TABLE context_traces;
                 DROP TABLE context_cache_boundary_items;
                 DROP TABLE context_cache_boundaries;
                 DROP TABLE context_plan_hashes;
                 DROP TABLE context_item_transforms;
                 DROP TABLE context_items;
                 DROP TABLE context_plans;
                 DROP TABLE continuity_receipt_refs;
                 DROP TABLE continuity_budget_events;
                 DROP TABLE continuity_migration_links;
                 DROP TABLE continuity_session_links;
                 DROP TABLE continuity_history_refs;
                 DROP TABLE continuity_activations;
                 DROP TABLE continuity_checkpoint_notes;
                 DROP TABLE continuity_checkpoint_evidence;
                 DROP TABLE continuity_checkpoint_sources;
                 DROP TABLE continuity_notes;
                 DROP TABLE continuity_checkpoints;
                 DROP TABLE continuity_epochs;
                 DROP TABLE continuity_meta;
                 DROP TABLE takeover_admission_anchors;
                 DROP TABLE takeover_admissions;
                 DROP TABLE entity_links;
                 DROP TABLE entities;
                 DROP TABLE memory_links;
                 DROP TABLE memory_anchors;
                 DROP TABLE memory_source_refs;
                 DROP TABLE org_usage_entries;
                 DROP TABLE org_sync_envelopes;
                 DROP TABLE org_policies;
                 DROP TABLE org_memberships;
                 DROP TABLE org_teams;
                 DROP TABLE org_accounts;
                 DROP TABLE organizations;
                 DROP TABLE worker_challenges;
                 DROP TABLE worker_jobs;
                 DROP TABLE worker_devices;
                 DROP TABLE routine_runs;
                 DROP TABLE routines;
                 DROP TABLE memories;
                 PRAGMA user_version = 12;",
            )
            .expect("simulate historical v12 schema");
    }

    let mut store = SqliteStore::open(&db.path).expect("migrate v12 to current");
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 37);
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let receipt_ref = PlanningCostReceiptRef::new("planning-receipt:after-upgrade").unwrap();
    let evidence = receipt(1);
    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("new table is usable after migration");
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).unwrap(),
        Some(evidence)
    );
}
