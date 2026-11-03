//! Runlith desktop host.
//!
//! A thin Tauri shell over agentd (ADR-0007): the webview never runs shell,
//! git, SQLite or file access. Every value it shows comes from agentd through
//! the commands in [`agentd`], mapped to UI DTOs in [`dto`]. Two things live in
//! the host itself because they are not agentd state: the overview layout file
//! ([`dashboard`]) and the live numbers its widgets show ([`live_metrics`]).

mod agentd;
mod dashboard;
mod dto;
mod live_metrics;
mod widgets;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let metrics = live_metrics::LiveMetrics::new(
                app.path().app_data_dir()?.join("metric-cache.json"),
            )?;
            app.manage(metrics);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agentd::connection_status,
            agentd::list_projects,
            agentd::list_tasks,
            agentd::list_dependencies,
            agentd::list_human_decisions,
            agentd::widget_draft_start,
            agentd::widget_draft_status,
            agentd::widget_draft_cancel,
            widgets::dashboard_load,
            widgets::dashboard_save,
            widgets::dashboard_stock,
            widgets::metric_read,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Runlith desktop host");
}
