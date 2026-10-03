//! F1: startup reconciliation of tasks stranded mid-chain. Only `SUBMITTED`
//! is routed (pure bookkeeping through the validated transition); states
//! that need a trusted verdict or a new execution are reported, never moved.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{FixedClock, TaskService};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("runlith-stranded-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn service(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(1_704_067_200_000)).expect("open service")
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

type Step = fn(&mut TaskService<FixedClock>, &TaskId);

/// Creates `id` in `project` and drives it to `target` through the
/// validated service operations.
fn task_in(
    service: &mut TaskService<FixedClock>,
    project: &str,
    id: &str,
    target: TaskState,
) -> TaskId {
    let project_id = ProjectId::new(project).unwrap();
    if service.store().get_project(&project_id).unwrap().is_none() {
        service
            .create_project(
                &Project::new(project_id.clone(), format!("project {project}")).unwrap(),
            )
            .unwrap();
    }
    let task = Task::draft(
        TaskId::new(id).unwrap(),
        project_id,
        format!("objective {id}"),
    )
    .unwrap();
    service.create_task(&task).unwrap();
    let id = task.id().clone();
    let steps: [(TaskState, Step); 7] = [
        (TaskState::Ready, |s, t| {
            s.mark_ready(t).unwrap();
        }),
        (TaskState::Claimed, |s, t| {
            s.claim(t).unwrap();
        }),
        (TaskState::Running, |s, t| {
            s.start(t).unwrap();
        }),
        (TaskState::Submitted, |s, t| {
            s.submit_result(t, "done").unwrap();
        }),
        (TaskState::AwaitingReview, |s, t| {
            s.route_to_review(t).unwrap();
        }),
        (TaskState::Verifying, |s, t| {
            s.begin_verification(t).unwrap();
        }),
        (TaskState::RetryReady, |s, t| {
            s.verification_failed(t, "REJECTED").unwrap();
        }),
    ];
    for (state, step) in steps {
        step(service, &id);
        if state == target {
            return id;
        }
    }
    id
}

fn state(service: &TaskService<FixedClock>, id: &TaskId) -> TaskState {
    service.get_task(id).unwrap().unwrap().state()
}

#[test]
fn sweep_routes_submitted_and_only_reports_the_rest() {
    let db = TestDb::new();
    let mut service = db.service();
    let submitted = task_in(&mut service, "P-1", "T-submitted", TaskState::Submitted);
    let awaiting = task_in(&mut service, "P-2", "T-awaiting", TaskState::AwaitingReview);
    let verifying = task_in(&mut service, "P-2", "T-verifying", TaskState::Verifying);
    let retry = task_in(&mut service, "P-1", "T-retry", TaskState::RetryReady);
    let running = task_in(&mut service, "P-1", "T-running", TaskState::Running);
    let ready = task_in(&mut service, "P-1", "T-ready", TaskState::Ready);

    let report = service.reconcile_stranded().unwrap();

    assert_eq!(report.routed, vec![submitted.clone()]);
    assert_eq!(state(&service, &submitted), TaskState::AwaitingReview);
    let mut reported: Vec<_> = report
        .needs_operator
        .iter()
        .map(|(id, s)| (id.clone(), *s))
        .collect();
    reported.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    // The freshly routed task is not re-reported in the same pass.
    assert_eq!(
        reported,
        vec![
            (awaiting.clone(), TaskState::AwaitingReview),
            (retry.clone(), TaskState::RetryReady),
            (verifying.clone(), TaskState::Verifying),
        ]
    );
    assert_eq!(state(&service, &awaiting), TaskState::AwaitingReview);
    assert_eq!(state(&service, &verifying), TaskState::Verifying);
    assert_eq!(state(&service, &retry), TaskState::RetryReady);
    assert_eq!(state(&service, &running), TaskState::Running);
    assert_eq!(state(&service, &ready), TaskState::Ready);

    let routed_event = service
        .list_events_for_task(&submitted)
        .unwrap()
        .into_iter()
        .last()
        .unwrap();
    assert_eq!(
        routed_event.fact.provenance,
        "task-application-service/reconcile/route_to_review"
    );
}

#[test]
fn sweep_is_idempotent() {
    let db = TestDb::new();
    let mut service = db.service();
    let submitted = task_in(&mut service, "P-1", "T-1", TaskState::Submitted);
    service.reconcile_stranded().unwrap();
    let events_after_first = service.store().list_events_ordered().unwrap().len();
    let second = service.reconcile_stranded().unwrap();
    assert!(second.routed.is_empty());
    assert_eq!(
        second.needs_operator,
        vec![(submitted, TaskState::AwaitingReview)]
    );
    assert_eq!(
        service.store().list_events_ordered().unwrap().len(),
        events_after_first
    );
}
