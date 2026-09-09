//! Build script for `ipc-contracts`.
//!
//! Compiles the canonical Protobuf wire contract
//! (`proto/agentd_control.proto`) with `prost-build` into Cargo `OUT_DIR`
//! using the vendored `protoc` binary. Generated Rust is derived build
//! output only and is never committed.

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
    prost_build::compile_protos(
        &["proto/agentd_control.proto", "proto/task_core.proto"],
        &["proto/"],
    )
    .expect("canonical proto must compile with prost-build");
}
