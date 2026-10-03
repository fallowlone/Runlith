//! Fail-closed stand-in for [`path_security`](crate) on non-Unix platforms.
//!
//! Owner, mode, hard-link and identity admission exists only for Unix. A
//! Windows ACL implementation is not provided, so opening the durable store
//! is refused instead of proceeding with unchecked path admission (F12).

use std::path::Path;

use crate::{StorageError, StorageResult};

/// Never constructed: [`prepare_database_path`] always refuses.
pub(crate) struct PreparedDatabasePath {
    path: std::path::PathBuf,
}

impl PreparedDatabasePath {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn prepare_database_path(path: &Path) -> StorageResult<PreparedDatabasePath> {
    Err(StorageError::UnsafePath(format!(
        "durable storage admission is not implemented on this platform: {}",
        path.display()
    )))
}

pub(crate) fn verify_database_path(_prepared: &PreparedDatabasePath) -> StorageResult<()> {
    Err(StorageError::UnsafePath(
        "durable storage admission is not implemented on this platform".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_every_database_path() {
        let result = prepare_database_path(Path::new("runlith.db"));
        assert!(matches!(result, Err(StorageError::UnsafePath(_))));
    }
}
