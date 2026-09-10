//! TASK-0046 liveness wiring: `GitCli::with_liveness` propagates a
//! provider-neutral [`workspace::OwnerLiveness`] answer through
//! `owner_is_live`; the unwired adapter keeps the contractual-honest G4
//! `false`.

use std::sync::Arc;

use git_cli::GitCli;
use workspace::{ExecutionId, GitPort, OwnerLiveness, RepositoryRef};

struct ScriptedLiveness {
    live: Vec<String>,
}

impl OwnerLiveness for ScriptedLiveness {
    fn is_live(&self, owner: &ExecutionId) -> bool {
        self.live.iter().any(|id| id == owner.as_str())
    }
}

fn repo_ref() -> RepositoryRef {
    RepositoryRef::new("repo-local:liveness").unwrap()
}

#[test]
fn unwired_adapter_honestly_reports_no_live_owner() {
    let adapter = GitCli::new();
    let owner = ExecutionId::new("E-live").unwrap();
    assert!(!adapter.owner_is_live(&repo_ref(), &owner).unwrap());
}

#[test]
fn wired_liveness_source_decides_per_owner() {
    let source: Arc<dyn OwnerLiveness> = Arc::new(ScriptedLiveness {
        live: vec!["E-live".to_owned()],
    });
    let adapter = GitCli::new().with_liveness(source);
    let live = ExecutionId::new("E-live").unwrap();
    let dead = ExecutionId::new("E-done").unwrap();
    assert!(adapter.owner_is_live(&repo_ref(), &live).unwrap());
    assert!(!adapter.owner_is_live(&repo_ref(), &dead).unwrap());
}
