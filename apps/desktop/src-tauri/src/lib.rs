//! Runlith desktop shell.
//!
//! ADR-0007: the frontend is a thin native shell. It must not execute
//! shell/process/Git operations and owns no business logic; agentd owns
//! local orchestration. This scaffold renders design fixtures only and
//! registers no commands or plugins yet — IPC read contracts are a
//! separate, explicitly scoped task (see ADR-0016).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running runlith desktop shell");
}