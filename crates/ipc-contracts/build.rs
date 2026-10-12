//! Build script for `ipc-contracts`.
//!
//! Compiles the canonical Protobuf wire contract
//! (`proto/agentd_control.proto`) with `prost-build` into Cargo `OUT_DIR`
//! using the vendored `protoc` binary. Generated Rust is derived build
//! output only and is never committed.

/// Kept in sync with the redacting `Debug` impls in `src/lib.rs`; a missing
/// impl fails the build, an extra one conflicts with the generated derive.
const SECRET_BEARING_MESSAGES: [&str; 7] = [
    "VaultCreateRequest",
    "VaultUnlockRequest",
    "VaultStoreRequest",
    "VaultChangePasswordRequest",
    "VaultRecoverRequest",
    "VaultRotateRequest",
    "VaultPhraseResponse",
];

fn main() {
    let protoc_path = protoc_bin_vendored::protoc_bin_path()
        .expect("vendored protoc binary must be available via protoc-bin-vendored");
    // `prost-build` honors `PROTOC`; pointing it at the vendored binary
    // keeps normal builds free of any globally installed `protoc`.
    unsafe {
        std::env::set_var("PROTOC", &protoc_path);
    }
    println!("cargo:rerun-if-changed=proto/agentd_control.proto");
    println!("cargo:rerun-if-changed=proto/task_core.proto");
    println!("cargo:rerun-if-changed=proto/execution_core.proto");
    println!("cargo:rerun-if-changed=proto/policy_core.proto");
    println!("cargo:rerun-if-changed=proto/human_decision_core.proto");
    println!("cargo:rerun-if-changed=proto/planning_core.proto");
    println!("cargo:rerun-if-changed=proto/task_authoring_core.proto");
    println!("cargo:rerun-if-changed=proto/vault_core.proto");
    println!("cargo:rerun-if-changed=proto/finder_core.proto");
    // Messages carrying vault secrets get a hand-written redacting `Debug`
    // in `src/lib.rs` instead of the generated one (ADR-0039).
    prost_build::Config::new()
        .skip_debug(
            SECRET_BEARING_MESSAGES
                .iter()
                .map(|name| format!(".ai_engineering_os.ipc.{name}")),
        )
        .compile_protos(
            &[
                "proto/agentd_control.proto",
                "proto/task_core.proto",
                "proto/execution_core.proto",
                "proto/policy_core.proto",
                "proto/human_decision_core.proto",
                "proto/planning_core.proto",
                "proto/task_authoring_core.proto",
                "proto/vault_core.proto",
                "proto/finder_core.proto",
            ],
            &["proto/"],
        )
        .expect("canonical proto must compile with prost-build");
}
