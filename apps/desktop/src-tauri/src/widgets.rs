//! Commands for the overview layout and its live numbers. Values from the
//! webview are parsed into the validated `widget_spec` types on the way in.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};
use widget_spec::{DashboardLayout, MetricSource, RefreshSeconds};

use crate::dashboard::{self, DashboardDto};
use crate::dto::{UiError, UiErrorKind};
use crate::live_metrics::{LiveMetrics, MetricDto};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, UiError> {
    app.path()
        .app_data_dir()
        .map_err(|error| UiError::new(UiErrorKind::Internal, error.to_string()))
}

#[tauri::command]
pub fn dashboard_load(app: AppHandle) -> Result<DashboardDto, UiError> {
    dashboard::load(&data_dir(&app)?, now_ms()).map_err(|error| {
        UiError::new(
            UiErrorKind::Internal,
            format!("could not read the layout: {error}"),
        )
    })
}

#[tauri::command]
pub fn dashboard_save(app: AppHandle, layout: DashboardLayout) -> Result<(), UiError> {
    layout
        .validate()
        .map_err(|error| UiError::new(UiErrorKind::Invalid, error.to_string()))?;
    dashboard::save(&data_dir(&app)?, &layout).map_err(|error| {
        UiError::new(
            UiErrorKind::Internal,
            format!("could not save the layout: {error}"),
        )
    })
}

#[tauri::command]
pub fn dashboard_stock() -> DashboardLayout {
    DashboardLayout::stock()
}

#[tauri::command]
pub async fn metric_read(
    metrics: State<'_, LiveMetrics>,
    source: MetricSource,
    every_seconds: RefreshSeconds,
) -> Result<MetricDto, UiError> {
    Ok(metrics.read(&source, every_seconds, now_ms()).await)
}
