use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn independent_owners_are_serialized_and_state_survives_reopen() {
    let dir = private_dir();
    let vault = dir.path().join("vault");
    let guard = Guard::open(&vault).unwrap();
    guard
        .save(State {
            failures: 8,
            retry_at_ms: 9000,
        })
        .unwrap();
    let error = match Guard::open(&vault) {
        Ok(_) => panic!("second owner acquired the lock"),
        Err(error) => error,
    };
    assert_eq!(error.code(), VaultErrorCode::VaultErrorRateLimited);
    drop(guard);
    let reopened = Guard::open(&vault).unwrap();
    let stored = reopened.load().unwrap();
    assert_eq!(stored.failures, 8);
    assert_eq!(stored.retry_at_ms, 9000);
}

#[test]
fn invalid_missing_and_unsafe_state_never_reset_the_counter() {
    for fault in 0..5 {
        let dir = private_dir();
        let vault = dir.path().join("vault");
        let guard = Guard::open(&vault).unwrap();
        guard
            .save(State {
                failures: 8,
                retry_at_ms: 9000,
            })
            .unwrap();
        drop(guard);
        let state = sidecar(&vault, ".unlock-state");
        let canary = dir.path().join("canary");
        fs::write(&canary, b"unchanged").unwrap();
        fs::set_permissions(&canary, fs::Permissions::from_mode(0o600)).unwrap();
        match fault {
            0 => fs::write(&state, b"invalid").unwrap(),
            1 => fs::remove_file(&state).unwrap(),
            2 => {
                fs::remove_file(&state).unwrap();
                symlink(&canary, &state).unwrap();
            }
            3 => {
                fs::remove_file(&state).unwrap();
                fs::hard_link(&canary, &state).unwrap();
            }
            4 => fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap(),
            _ => unreachable!(),
        }
        assert!(Guard::open(&vault).is_err(), "fault {fault}");
        assert_eq!(fs::read(&canary).unwrap(), b"unchanged");
    }
}

#[test]
fn unsafe_lock_and_parent_are_refused() {
    let dir = private_dir();
    let vault = dir.path().join("vault");
    let canary = dir.path().join("canary");
    fs::write(&canary, b"unchanged").unwrap();
    symlink(&canary, sidecar(&vault, ".unlock.lock")).unwrap();
    assert!(Guard::open(&vault).is_err());
    assert_eq!(fs::read(&canary).unwrap(), b"unchanged");
    fs::remove_file(sidecar(&vault, ".unlock.lock")).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Guard::open(&vault).is_err());
}

/// A flock belongs to the open file description, and `fork` shares it with the
/// child until `exec` closes the close-on-exec copy. Dropping the guard must
/// therefore unlock explicitly, or a concurrent spawn keeps the lock alive past
/// the drop for as long as the child lingers between fork and exec.
#[test]
fn dropped_guard_releases_the_lock_while_a_forked_child_still_holds_the_descriptor() {
    use std::os::unix::process::CommandExt;
    let dir = private_dir();
    let vault = dir.path().join("vault");
    let guard = Guard::open(&vault).unwrap();
    let spawner = std::thread::spawn(|| {
        let mut command = std::process::Command::new("true");
        // SAFETY: the closure only sleeps, which is async-signal-safe.
        unsafe {
            command.pre_exec(|| {
                // Far longer than the bounded retry in `Guard::open`.
                std::thread::sleep(std::time::Duration::from_millis(1500));
                Ok(())
            });
        }
        command.status().unwrap();
    });
    // Let the fork happen; the child now holds the lock descriptor.
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(guard);
    let reopened = Guard::open(&vault);
    spawner.join().unwrap();
    assert!(
        reopened.is_ok(),
        "lock stayed held by the forked child after the guard was dropped"
    );
}
