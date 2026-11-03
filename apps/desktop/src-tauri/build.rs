fn main() {
    // An app manifest makes every command opt-in: the webview can only call
    // what capabilities/default.json grants.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "connection_status",
            "list_projects",
            "list_tasks",
            "list_dependencies",
            "list_human_decisions",
            "widget_draft_start",
            "widget_draft_status",
            "widget_draft_cancel",
            "dashboard_load",
            "dashboard_save",
            "dashboard_stock",
            "metric_read",
        ]),
    ))
    .expect("failed to run tauri-build");
}
