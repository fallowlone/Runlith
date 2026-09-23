use effect_capture_contract::run_effect_capture_contract;
use effect_capture_local::LocalEffectCapture;
use evaluation::{AttemptId, ExecutionId};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn cv_2_cv_3_local_adapter_satisfies_shared_capture_contract() {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "runlith-effect-contract-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let capture = LocalEffectCapture::default();
    run_effect_capture_contract(
        &capture,
        &root,
        &ExecutionId::new("E-CONTRACT").unwrap(),
        &AttemptId::new("A-CONTRACT").unwrap(),
    );
    let _ = std::fs::remove_dir_all(root);
}
