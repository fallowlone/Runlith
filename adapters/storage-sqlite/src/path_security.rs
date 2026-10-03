//! Fail-closed admission for the durable SQLite file and its sidecars.
//!
//! Production creates/validates the application data directory in `agentd`
//! before this adapter is opened. This layer still protects every open from
//! final-component symlinks, insecure database/sidecar modes, path replacement
//! between admission and SQLite open, and umask-dependent database creation.
//!
//! Unix only. Other platforms compile `path_security_unsupported.rs`, which
//! refuses every open: owner/mode/ACL admission is not implemented there.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::{StorageError, StorageResult};

const PRIVATE_FILE_MODE: u32 = 0o600;

const SQLITE_SIDECARS: &[&str] = &["-journal", "-wal", "-shm"];

pub(crate) struct PreparedDatabasePath {
    path: PathBuf,
    parent_uid: u32,
    identity: FileIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl PreparedDatabasePath {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn prepare_database_path(path: &Path) -> StorageResult<PreparedDatabasePath> {
    let file_name = path
        .file_name()
        .ok_or_else(|| unsafe_path(format!("database path {} has no file name", path.display())))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = match parent {
        Some(parent) => parent.to_path_buf(),
        None => {
            std::env::current_dir().map_err(|error| io_path("resolve current directory", error))?
        }
    };

    let parent_metadata = fs::symlink_metadata(&parent).map_err(|error| {
        io_path(
            &format!("inspect database parent {}", parent.display()),
            error,
        )
    })?;
    if parent_metadata.file_type().is_symlink() {
        return Err(unsafe_path(format!(
            "database parent {} is a symlink",
            parent.display()
        )));
    }
    if !parent_metadata.is_dir() {
        return Err(unsafe_path(format!(
            "database parent {} is not a directory",
            parent.display()
        )));
    }
    validate_parent_metadata(&parent, &parent_metadata)?;

    let canonical_parent = fs::canonicalize(&parent).map_err(|error| {
        io_path(
            &format!("canonicalize database parent {}", parent.display()),
            error,
        )
    })?;
    let canonical_parent_metadata = fs::symlink_metadata(&canonical_parent).map_err(|error| {
        io_path(
            &format!(
                "inspect canonical database parent {}",
                canonical_parent.display()
            ),
            error,
        )
    })?;
    validate_parent_metadata(&canonical_parent, &canonical_parent_metadata)?;
    let canonical_path = canonical_parent.join(file_name);

    ensure_database_file(&canonical_path)?;
    let (identity, parent_uid) = {
        let metadata =
            validate_private_regular_file(&canonical_path, canonical_parent_metadata.uid())?;
        validate_sidecars(&canonical_path, canonical_parent_metadata.uid())?;
        (
            FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            canonical_parent_metadata.uid(),
        )
    };

    Ok(PreparedDatabasePath {
        path: canonical_path,
        parent_uid,
        identity,
    })
}

pub(crate) fn verify_database_path(prepared: &PreparedDatabasePath) -> StorageResult<()> {
    {
        let metadata = validate_private_regular_file(&prepared.path, prepared.parent_uid)?;
        let observed = FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        if observed != prepared.identity {
            return Err(unsafe_path(format!(
                "database file {} changed identity during open",
                prepared.path.display()
            )));
        }
        validate_sidecars(&prepared.path, prepared.parent_uid)?;
    }
    Ok(())
}

fn ensure_database_file(path: &Path) -> StorageResult<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(io_path(
                &format!("inspect database file {}", path.display()),
                error,
            ))
        }
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.mode(PRIVATE_FILE_MODE);
    match options.open(path) {
        Ok(file) => {
            file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
                .map_err(|error| {
                    io_path(
                        &format!("secure freshly created database {}", path.display()),
                        error,
                    )
                })?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(io_path(
            &format!("create database file {}", path.display()),
            error,
        )),
    }
}

fn validate_parent_metadata(path: &Path, metadata: &fs::Metadata) -> StorageResult<()> {
    if metadata.mode() & 0o022 != 0 {
        return Err(unsafe_path(format!(
            "database parent {} is group/world writable",
            path.display()
        )));
    }
    Ok(())
}

fn validate_private_regular_file(path: &Path, parent_uid: u32) -> StorageResult<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_path(&format!("inspect sqlite file {}", path.display()), error))?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(unsafe_path(format!(
            "sqlite file {} is a symlink",
            path.display()
        )));
    }
    if !file_type.is_file() {
        return Err(unsafe_path(format!(
            "sqlite path {} is not a regular file",
            path.display()
        )));
    }
    if metadata.uid() != parent_uid {
        return Err(unsafe_path(format!(
            "sqlite file {} is not owned by the database-directory owner",
            path.display()
        )));
    }
    if metadata.mode() & 0o7777 != PRIVATE_FILE_MODE {
        return Err(unsafe_path(format!(
            "sqlite file {} must have mode 0600",
            path.display()
        )));
    }
    if metadata.nlink() != 1 {
        return Err(unsafe_path(format!(
            "sqlite file {} must have exactly one hard link",
            path.display()
        )));
    }
    Ok(metadata)
}

fn validate_sidecars(path: &Path, parent_uid: u32) -> StorageResult<()> {
    for suffix in SQLITE_SIDECARS {
        let sidecar = sidecar_path(path, suffix);
        match fs::symlink_metadata(&sidecar) {
            Ok(_) => {
                validate_private_regular_file(&sidecar, parent_uid)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io_path(
                    &format!("inspect sqlite sidecar {}", sidecar.display()),
                    error,
                ))
            }
        }
    }
    Ok(())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn unsafe_path(detail: String) -> StorageError {
    StorageError::UnsafePath(detail)
}

fn io_path(action: &str, error: io::Error) -> StorageError {
    StorageError::UnsafePath(format!("{action}: {error}"))
}
