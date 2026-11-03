//! The overview layout on disk (TASK-0230): one versioned JSON file in the app
//! data directory. Writes are atomic, so a crash leaves the old file or the new
//! one. A file that fails validation is moved aside, never overwritten, and the
//! stock layout takes its place.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;
use widget_spec::DashboardLayout;

const FILE_NAME: &str = "dashboard.json";
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DashboardDto {
    pub layout: DashboardLayout,
    /// Name of the copy kept when the saved file could not be read.
    pub recovered_copy: Option<String>,
}

/// Temp file, flush, rename, flush the directory: all or nothing.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("path has no file name"))?;
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

pub fn load(dir: &Path, now_ms: u64) -> io::Result<DashboardDto> {
    let path = dir.join(FILE_NAME);
    let stock = |recovered_copy| DashboardDto {
        layout: DashboardLayout::stock(),
        recovered_copy,
    };
    let size = match std::fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(stock(None)),
        Err(error) => return Err(error),
    };
    let parsed = if size > MAX_FILE_BYTES {
        None
    } else {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| widget_spec::parse_layout(&text).ok())
    };
    if let Some(layout) = parsed {
        return Ok(DashboardDto {
            layout,
            recovered_copy: None,
        });
    }
    let copy = format!("dashboard.corrupt-{now_ms}.json");
    std::fs::rename(&path, dir.join(&copy))?;
    Ok(stock(Some(copy)))
}

pub fn save(dir: &Path, layout: &DashboardLayout) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(layout).map_err(io::Error::other)?;
    write_atomic(&dir.join(FILE_NAME), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use widget_spec::{MetricSource, Motion, Refresh, SourceType, Widget};

    fn with_metric() -> DashboardLayout {
        let mut layout = DashboardLayout::stock();
        layout.widgets.push(Widget::Metric {
            id: "w-stars".to_owned().try_into().expect("id"),
            span: 2.try_into().expect("span"),
            source: MetricSource {
                kind: SourceType::GithubRepoStars,
                repo: "cli/cli".to_owned().try_into().expect("repo"),
            },
            refresh: Refresh {
                every_seconds: 300.try_into().expect("refresh"),
            },
            motion: Motion::Roll,
        });
        layout
    }

    #[test]
    fn a_first_run_gets_the_stock_layout_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let loaded = load(dir.path(), 1).expect("load");
        assert_eq!(loaded.layout, DashboardLayout::stock());
        assert_eq!(loaded.recovered_copy, None);
        assert!(!dir.path().join(FILE_NAME).exists());
    }

    #[test]
    fn a_saved_layout_is_read_back_exactly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = with_metric();
        save(dir.path(), &layout).expect("save");
        let loaded = load(dir.path(), 1).expect("load");
        assert_eq!(loaded.layout, layout);
        assert_eq!(loaded.recovered_copy, None);
    }

    #[test]
    fn a_corrupt_file_gives_the_stock_layout_and_its_bytes_are_kept() {
        for broken in [
            "{ not json",
            r#"{"version":1,"widgets":[{"kind":"metric","id":"x","span":3}]}"#,
            r#"{"version":99,"widgets":[]}"#,
            r#"{"version":1,"widgets":[],"script":"alert(1)"}"#,
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join(FILE_NAME), broken).expect("write");
            let loaded = load(dir.path(), 77).expect("load");
            assert_eq!(loaded.layout, DashboardLayout::stock(), "{broken}");
            let copy = loaded.recovered_copy.expect("copy name");
            assert_eq!(copy, "dashboard.corrupt-77.json");
            assert_eq!(
                std::fs::read_to_string(dir.path().join(copy)).expect("copy"),
                broken
            );
            assert!(!dir.path().join(FILE_NAME).exists());
        }
    }

    #[test]
    fn an_oversized_file_is_moved_aside_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = vec![b' '; (MAX_FILE_BYTES + 1) as usize];
        std::fs::write(dir.path().join(FILE_NAME), big).expect("write");
        let loaded = load(dir.path(), 5).expect("load");
        assert!(loaded.recovered_copy.is_some());
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_in_place() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = with_metric();
        save(dir.path(), &layout).expect("save");
        // A directory where the temp file belongs makes the write fail.
        let temp = dir
            .path()
            .join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
        std::fs::create_dir(&temp).expect("block temp");
        assert!(save(dir.path(), &DashboardLayout::stock()).is_err());
        assert_eq!(load(dir.path(), 1).expect("load").layout, layout);
    }

    #[test]
    fn the_saved_file_is_private_to_this_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        save(dir.path(), &with_metric()).expect("save");
        let mode = std::fs::metadata(dir.path().join(FILE_NAME))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
