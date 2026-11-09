use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A private scratch directory that removes itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rl-ps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// SQLite creates and deletes `<db>-journal` on every transaction of another
/// connection, so a sidecar seen by the first stat can be gone by the time it
/// is validated. That is normal and must not read as an unsafe path.
#[test]
fn a_sidecar_that_vanishes_during_admission_is_not_an_unsafe_path() {
    let dir = Scratch::new("vanish");
    let database = dir.0.join("state.sqlite3");
    fs::write(&database, b"").unwrap();
    let uid = fs::metadata(&dir.0).unwrap().uid();
    let journal = sidecar_path(&database, "-journal");

    let stop = Arc::new(AtomicBool::new(false));
    let churn = {
        let stop = Arc::clone(&stop);
        let journal = journal.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(PRIVATE_FILE_MODE)
                    .open(&journal)
                    .is_ok()
                {
                    let _ = fs::remove_file(&journal);
                }
            }
        })
    };
    let mut failure = None;
    for _ in 0..20_000 {
        if let Err(error) = validate_sidecars(&database, uid) {
            failure = Some(error.to_string());
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    churn.join().unwrap();
    assert_eq!(failure, None);
}

#[test]
fn a_sidecar_that_stays_with_a_loose_mode_is_still_refused() {
    let dir = Scratch::new("loose");
    let database = dir.0.join("state.sqlite3");
    fs::write(&database, b"").unwrap();
    let uid = fs::metadata(&dir.0).unwrap().uid();
    let journal = sidecar_path(&database, "-journal");
    fs::write(&journal, b"x").unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        validate_sidecars(&database, uid),
        Err(StorageError::UnsafePath(_))
    ));
}
