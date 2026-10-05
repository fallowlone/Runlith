//! Descriptor-relative candidate snapshot I/O (ADR-0035).

use evaluation::CandidateFile;
#[cfg(target_os = "macos")]
use evaluation::{MAX_CANDIDATE_BLOB_BYTES, MAX_CANDIDATE_ENTRIES};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSnapshot {
    pub files: Vec<CandidateFile>,
    pub directories: Vec<Vec<u8>>,
    pub git_pointer: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorReadOperation {
    OpenDirectory,
    ReadDirectory,
    Metadata,
    OpenFile,
    ReadFile,
}

#[cfg(target_os = "macos")]
type ReadAuthorizer<'a> =
    dyn FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError> + 'a;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::{c_char, c_int, c_void, CString, OsString};
    use std::fs::{File, Permissions};
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    const AT_FDCWD: c_int = -2;
    const O_RDONLY: c_int = 0;
    const O_WRONLY: c_int = 1;
    const O_CREAT: c_int = 0x0200;
    const O_EXCL: c_int = 0x0800;
    const O_NOFOLLOW: c_int = 0x0100;
    const O_NONBLOCK: c_int = 0x0004; // Darwin <sys/fcntl.h>
    const O_DIRECTORY: c_int = 0x100000;
    const O_CLOEXEC: c_int = 0x1000000;
    const AT_REMOVEDIR: c_int = 0x80;

    #[repr(C, packed(4))]
    struct Dirent {
        _inode: u64,
        _seek_offset: u64,
        _record_length: u16,
        name_length: u16,
        _file_type: u8,
        name: [c_char; 1024],
    }

    unsafe extern "C" {
        fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        fn mkdirat(dirfd: c_int, path: *const c_char, mode: u16) -> c_int;
        fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
        fn dup(fd: c_int) -> c_int;
        fn fdopendir(fd: c_int) -> *mut c_void;
        fn readdir(directory: *mut c_void) -> *const Dirent;
        fn closedir(directory: *mut c_void) -> c_int;
        fn __error() -> *mut c_int;
    }

    pub(super) fn materialize(
        parent: &Path,
        name: &str,
        files: &[CandidateFile],
    ) -> Result<PathBuf, evaluation::EffectError> {
        validate_component(name)?;
        validate_files(files)?;
        let parent = parent
            .canonicalize()
            .map_err(|_| failed("snapshot parent is unavailable"))?;
        let parent_file = open_directory(&parent)?;
        let snapshots = CString::new("candidate-snapshots").expect("static component");
        let result = unsafe { mkdirat(parent_file.as_raw_fd(), snapshots.as_ptr(), 0o700) };
        if result != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(failed("snapshot directory creation failed"));
        }
        let snapshots_file = open_child_directory(&parent_file, "candidate-snapshots")?;
        let name_c = component(name)?;
        if unsafe { mkdirat(snapshots_file.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
            return Err(failed(
                "candidate snapshot already exists or cannot be created",
            ));
        }
        let root = open_child_directory(&snapshots_file, name)?;

        for candidate in files {
            let parts = candidate
                .path
                .split(|byte| *byte == b'/')
                .map(std::str::from_utf8)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| failed("snapshot path is not supported"))?;
            let mut current = root
                .try_clone()
                .map_err(|_| failed("snapshot root unavailable"))?;
            for part in &parts[..parts.len() - 1] {
                let cpart = component(part)?;
                if unsafe { mkdirat(current.as_raw_fd(), cpart.as_ptr(), 0o700) } != 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(failed("snapshot subdirectory creation failed"));
                }
                current = open_child_directory(&current, part)?;
            }
            let leaf = parts.last().ok_or_else(|| failed("empty snapshot path"))?;
            let leaf = component(leaf)?;
            let fd = unsafe {
                openat(
                    current.as_raw_fd(),
                    leaf.as_ptr(),
                    O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(failed("snapshot file creation failed"));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(&candidate.bytes)
                .map_err(|_| failed("snapshot file write failed"))?;
            file.sync_all()
                .map_err(|_| failed("snapshot file sync failed"))?;
            file.set_permissions(Permissions::from_mode(if candidate.mode == 0o100755 {
                0o555
            } else {
                0o444
            }))
            .map_err(|_| failed("snapshot file sealing failed"))?;
        }
        seal_directories(&root)?;
        root.sync_all()
            .map_err(|_| failed("snapshot root sync failed"))?;
        Ok(parent.join("candidate-snapshots").join(name))
    }

    fn seal_directories(dir: &File) -> Result<(), evaluation::EffectError> {
        for name in read_directory_names(dir, b".", None)? {
            let name = name
                .to_str()
                .ok_or_else(|| failed("snapshot path is not supported"))?;
            let child = open_child_directory(dir, name);
            if let Ok(child) = child {
                seal_directories(&child)?;
                child
                    .set_permissions(Permissions::from_mode(0o555))
                    .map_err(|_| failed("snapshot directory sealing failed"))?;
            } else {
                let component = component(name)?;
                // O_NONBLOCK ensures special files (FIFO, device) fail promptly
                // on open rather than blocking forever; ADR-0035 requires checks
                // to fail closed, never hang.
                let fd = unsafe {
                    openat(
                        dir.as_raw_fd(),
                        component.as_ptr(),
                        O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                    )
                };
                if fd < 0 {
                    return Err(failed("snapshot contains unsupported entry"));
                }
                let file = unsafe { File::from_raw_fd(fd) };
                if !file
                    .metadata()
                    .map_err(|_| failed("snapshot file metadata unavailable"))?
                    .is_file()
                {
                    return Err(failed("snapshot contains unsupported entry"));
                }
            }
        }
        dir.set_permissions(Permissions::from_mode(0o555))
            .map_err(|_| failed("snapshot root sealing failed"))
    }

    pub(super) fn read(root: &Path) -> Result<CandidateSnapshot, evaluation::EffectError> {
        let root = root
            .canonicalize()
            .map_err(|_| failed("candidate snapshot is unavailable"))?;
        let root_file = open_directory(&root)?;
        if root_file
            .metadata()
            .map_err(|_| failed("candidate snapshot metadata unavailable"))?
            .permissions()
            .mode()
            & 0o777
            != 0o555
        {
            return Err(failed("candidate snapshot root is not sealed"));
        }
        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut git_pointer = None;
        let mut total = 0u64;
        WalkState {
            files: &mut files,
            directories: &mut directories,
            allow_root_git_pointer: false,
            require_sealed: true,
            git_pointer: &mut git_pointer,
            total: &mut total,
            authorize: &mut |_, _| Ok(()),
        }
        .walk(&root_file, &mut Vec::new(), 0)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        directories.sort();
        validate_files(&files)?;
        Ok(CandidateSnapshot {
            files,
            directories,
            git_pointer,
        })
    }

    pub(super) fn remove(parent: &Path, name: &str) -> Result<(), evaluation::EffectError> {
        validate_component(name)?;
        let parent = parent
            .canonicalize()
            .map_err(|_| failed("snapshot parent is unavailable"))?;
        let parent_file = open_directory(&parent)?;
        let snapshots = open_child_directory(&parent_file, "candidate-snapshots")?;
        let root = open_child_directory(&snapshots, name)?;
        remove_contents(&root)?;
        let name = component(name)?;
        if unsafe { unlinkat(snapshots.as_raw_fd(), name.as_ptr(), AT_REMOVEDIR) } != 0 {
            return Err(failed("candidate snapshot removal failed"));
        }
        snapshots
            .sync_all()
            .map_err(|_| failed("candidate snapshot removal sync failed"))
    }

    fn remove_contents(directory: &File) -> Result<(), evaluation::EffectError> {
        directory
            .set_permissions(Permissions::from_mode(0o700))
            .map_err(|_| failed("candidate snapshot directory removal refused"))?;
        for name in read_directory_names(directory, b".", None)? {
            let component_text = name
                .to_str()
                .ok_or_else(|| failed("candidate snapshot entry name is unsupported"))?;
            validate_component(component_text)?;
            let component = component(component_text)?;
            let child_fd = unsafe {
                openat(
                    directory.as_raw_fd(),
                    component.as_ptr(),
                    O_RDONLY | O_NOFOLLOW | O_DIRECTORY | O_CLOEXEC | O_NONBLOCK,
                )
            };
            if child_fd >= 0 {
                let child = unsafe { File::from_raw_fd(child_fd) };
                remove_contents(&child)?;
                if unsafe { unlinkat(directory.as_raw_fd(), component.as_ptr(), AT_REMOVEDIR) } != 0
                {
                    return Err(failed("candidate snapshot directory removal failed"));
                }
                continue;
            }
            let file_fd = unsafe {
                openat(
                    directory.as_raw_fd(),
                    component.as_ptr(),
                    O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                )
            };
            if file_fd < 0 {
                return Err(failed("candidate snapshot contains unsupported entry"));
            }
            let file = unsafe { File::from_raw_fd(file_fd) };
            if !file
                .metadata()
                .map_err(|_| failed("candidate snapshot entry metadata unavailable"))?
                .is_file()
            {
                return Err(failed("candidate snapshot contains unsupported entry"));
            }
            if unsafe { unlinkat(directory.as_raw_fd(), component.as_ptr(), 0) } != 0 {
                return Err(failed("candidate snapshot file removal failed"));
            }
        }
        Ok(())
    }

    pub(super) fn scan_live_worktree(
        root: &Path,
        authorize: &mut impl FnMut(
            &[u8],
            DescriptorReadOperation,
        ) -> Result<(), evaluation::EffectError>,
    ) -> Result<CandidateSnapshot, evaluation::EffectError> {
        authorize(b".", DescriptorReadOperation::OpenDirectory)?;
        let authorized_root = root;
        let root = authorized_root
            .canonicalize()
            .map_err(|_| failed("linked worktree is unavailable"))?;
        if root != authorized_root {
            return Err(failed("linked worktree path changed during authorization"));
        }
        let root_file = open_directory(&root)?;
        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut git_pointer = None;
        let mut total = 0u64;
        WalkState {
            files: &mut files,
            directories: &mut directories,
            allow_root_git_pointer: true,
            require_sealed: false,
            git_pointer: &mut git_pointer,
            total: &mut total,
            authorize,
        }
        .walk(&root_file, &mut Vec::new(), 0)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        directories.sort();
        validate_files(&files)?;
        if git_pointer.is_none() {
            return Err(failed("linked worktree .git pointer is missing"));
        }
        Ok(CandidateSnapshot {
            files,
            directories,
            git_pointer,
        })
    }

    pub(super) fn read_worktree_git_pointer(
        root: &Path,
        authorize: &mut impl FnMut(
            &[u8],
            DescriptorReadOperation,
        ) -> Result<(), evaluation::EffectError>,
    ) -> Result<Vec<u8>, evaluation::EffectError> {
        authorize(b".", DescriptorReadOperation::OpenDirectory)?;
        let authorized_root = root;
        let root = authorized_root
            .canonicalize()
            .map_err(|_| failed("linked worktree is unavailable"))?;
        if root != authorized_root {
            return Err(failed("linked worktree path changed during authorization"));
        }
        let root_file = open_directory(&root)?;
        authorize(b".git", DescriptorReadOperation::OpenFile)?;
        let name = CString::new(".git").expect("static component");
        let fd = unsafe {
            openat(
                root_file.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(failed("linked worktree .git pointer is unavailable"));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        authorize(b".git", DescriptorReadOperation::Metadata)?;
        if !file
            .metadata()
            .map_err(|_| failed("linked worktree .git pointer metadata unavailable"))?
            .is_file()
        {
            return Err(failed("linked worktree .git entry is not a pointer file"));
        }
        let mut bytes = Vec::new();
        read_authorized(&file, b".git", authorize, 4097, &mut bytes)?;
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(failed("linked worktree .git pointer has invalid size"));
        }
        Ok(bytes)
    }

    pub(super) fn read_linked_metadata_file(
        common_dir: &Path,
        relative_path: &[u8],
        authorize: &mut impl FnMut(
            &[u8],
            DescriptorReadOperation,
        ) -> Result<(), evaluation::EffectError>,
    ) -> Result<Vec<u8>, evaluation::EffectError> {
        read_linked_metadata_file_if_present(common_dir, relative_path, authorize)?
            .ok_or_else(|| failed("linked metadata file is absent"))
    }

    pub(super) fn read_linked_metadata_file_if_present(
        common_dir: &Path,
        relative_path: &[u8],
        authorize: &mut impl FnMut(
            &[u8],
            DescriptorReadOperation,
        ) -> Result<(), evaluation::EffectError>,
    ) -> Result<Option<Vec<u8>>, evaluation::EffectError> {
        validate_name_path(relative_path)?;
        authorize(b".", DescriptorReadOperation::OpenDirectory)?;
        let authorized_root = common_dir;
        let root = authorized_root
            .canonicalize()
            .map_err(|_| failed("trusted common Git directory is unavailable"))?;
        if root != authorized_root {
            return Err(failed("trusted common Git directory changed"));
        }
        let root_file = open_directory(&root)?;
        let parts = relative_path
            .split(|byte| *byte == b'/')
            .collect::<Vec<_>>();
        let mut current = root_file;
        let mut prefix = Vec::new();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push(b'/');
            }
            prefix.extend_from_slice(part);
            authorize(&prefix, DescriptorReadOperation::OpenDirectory)?;
            let name = std::str::from_utf8(part)
                .map_err(|_| failed("linked metadata path is unsupported"))?;
            current = open_child_directory(&current, name)?;
            authorize(&prefix, DescriptorReadOperation::Metadata)?;
            if !current
                .metadata()
                .map_err(|_| failed("linked metadata directory metadata unavailable"))?
                .is_dir()
            {
                return Err(failed("linked metadata path is not a directory"));
            }
        }
        let leaf = parts
            .last()
            .ok_or_else(|| failed("linked metadata path is empty"))?;
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        prefix.extend_from_slice(leaf);
        authorize(&prefix, DescriptorReadOperation::OpenFile)?;
        let leaf = component(
            std::str::from_utf8(leaf)
                .map_err(|_| failed("linked metadata filename is unsupported"))?,
        )?;
        let fd = unsafe {
            openat(
                current.as_raw_fd(),
                leaf.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
            )
        };
        if fd < 0 {
            if unsafe { *__error() } == 2 {
                return Ok(None);
            }
            return Err(failed("linked metadata file open failed"));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        authorize(&prefix, DescriptorReadOperation::Metadata)?;
        if !file
            .metadata()
            .map_err(|_| failed("linked metadata file metadata unavailable"))?
            .is_file()
        {
            return Err(failed("linked metadata entry is not a regular file"));
        }
        let mut bytes = Vec::new();
        read_authorized(&file, &prefix, authorize, 4097, &mut bytes)?;
        if bytes.len() > 4096 {
            return Err(failed("linked metadata file size is invalid"));
        }
        Ok(Some(bytes))
    }

    struct WalkState<'a> {
        files: &'a mut Vec<CandidateFile>,
        directories: &'a mut Vec<Vec<u8>>,
        allow_root_git_pointer: bool,
        require_sealed: bool,
        git_pointer: &'a mut Option<Vec<u8>>,
        total: &'a mut u64,
        authorize: &'a mut ReadAuthorizer<'a>,
    }

    impl WalkState<'_> {
        fn walk(
            &mut self,
            directory: &File,
            prefix: &mut Vec<u8>,
            depth: usize,
        ) -> Result<(), evaluation::EffectError> {
            if depth > evaluation::MAX_CANDIDATE_TREE_DEPTH
                || self.files.len() > MAX_CANDIDATE_ENTRIES
            {
                return Err(failed("candidate snapshot exceeded bound"));
            }
            (self.authorize)(prefix, DescriptorReadOperation::ReadDirectory)?;
            let mut entries = read_directory_names(directory, prefix, Some(self.authorize))?;
            entries.sort();
            for name in entries {
                let raw_name = name.as_bytes();
                if depth == 0 && raw_name == b".git" {
                    if !self.allow_root_git_pointer || self.git_pointer.is_some() {
                        return Err(failed("candidate snapshot contains unsupported .git entry"));
                    }
                    (self.authorize)(b".git", DescriptorReadOperation::OpenFile)?;
                    let c_name = CString::new(".git").expect("static component");
                    let fd = unsafe {
                        openat(
                            directory.as_raw_fd(),
                            c_name.as_ptr(),
                            O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                        )
                    };
                    if fd < 0 {
                        return Err(failed("linked worktree .git pointer is unavailable"));
                    }
                    let file = unsafe { File::from_raw_fd(fd) };
                    (self.authorize)(b".git", DescriptorReadOperation::Metadata)?;
                    if !file
                        .metadata()
                        .map_err(|_| failed("linked worktree .git pointer metadata unavailable"))?
                        .is_file()
                    {
                        return Err(failed("linked worktree .git entry is not a pointer file"));
                    }
                    let mut bytes = Vec::new();
                    read_authorized(&file, b".git", self.authorize, 4097, &mut bytes)?;
                    if bytes.is_empty() || bytes.len() > 4096 {
                        return Err(failed("linked worktree .git pointer has invalid size"));
                    }
                    *self.git_pointer = Some(bytes);
                    continue;
                }
                validate_name(raw_name)?;
                let mut path = prefix.clone();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(raw_name);
                if path.len() > 4096 {
                    return Err(failed("candidate snapshot path exceeded bound"));
                }
                (self.authorize)(&path, DescriptorReadOperation::OpenDirectory)?;
                let child = open_child_directory(
                    directory,
                    name.to_str()
                        .ok_or_else(|| failed("candidate snapshot path is not supported"))?,
                );
                if let Ok(child) = child {
                    (self.authorize)(&path, DescriptorReadOperation::Metadata)?;
                    let metadata = child
                        .metadata()
                        .map_err(|_| failed("candidate snapshot directory metadata unavailable"))?;
                    if !metadata.is_dir() {
                        return Err(failed("candidate snapshot entry is not a directory"));
                    }
                    if self.require_sealed && metadata.permissions().mode() & 0o777 != 0o555 {
                        return Err(failed("candidate snapshot directory is not sealed"));
                    }
                    prefix.extend_from_slice(if prefix.is_empty() { b"" } else { b"/" });
                    prefix.extend_from_slice(raw_name);
                    self.directories.push(prefix.clone());
                    self.walk(&child, prefix, depth + 1)?;
                    prefix.truncate(prefix.len() - raw_name.len());
                    if prefix.last() == Some(&b'/') {
                        prefix.pop();
                    }
                } else {
                    let c_name = component(
                        name.to_str()
                            .ok_or_else(|| failed("candidate snapshot path is not supported"))?,
                    )?;
                    (self.authorize)(&path, DescriptorReadOperation::OpenFile)?;
                    let fd = unsafe {
                        openat(
                            directory.as_raw_fd(),
                            c_name.as_ptr(),
                            O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                        )
                    };
                    if fd < 0 {
                        return Err(failed("candidate snapshot file open failed"));
                    }
                    let file = unsafe { File::from_raw_fd(fd) };
                    (self.authorize)(&path, DescriptorReadOperation::Metadata)?;
                    let metadata = file
                        .metadata()
                        .map_err(|_| failed("candidate snapshot file metadata unavailable"))?;
                    if !metadata.is_file() {
                        return Err(failed("candidate snapshot contains unsupported entry"));
                    }
                    let file_mode = metadata.permissions().mode() & 0o777;
                    if self.require_sealed && file_mode != 0o444 && file_mode != 0o555 {
                        return Err(failed("candidate snapshot file is not sealed"));
                    }
                    let mut bytes = Vec::new();
                    read_authorized(
                        &file,
                        &path,
                        self.authorize,
                        MAX_CANDIDATE_BLOB_BYTES + 1,
                        &mut bytes,
                    )?;
                    *self.total = self
                        .total
                        .checked_add(bytes.len() as u64)
                        .ok_or_else(|| failed("candidate snapshot exceeded byte bound"))?;
                    if *self.total > MAX_CANDIDATE_BLOB_BYTES {
                        return Err(failed("candidate snapshot exceeded byte bound"));
                    }
                    self.files.push(CandidateFile {
                        path,
                        mode: if file_mode & 0o111 != 0 {
                            0o100755
                        } else {
                            0o100644
                        },
                        git_oid: None,
                        bytes,
                    });
                }
                if self.files.len() > MAX_CANDIDATE_ENTRIES {
                    return Err(failed("candidate snapshot exceeded entry bound"));
                }
            }
            Ok(())
        }
    }

    fn read_directory_names(
        directory: &File,
        path: &[u8],
        mut authorize: Option<&mut ReadAuthorizer<'_>>,
    ) -> Result<Vec<OsString>, evaluation::EffectError> {
        let duplicate = unsafe { dup(directory.as_raw_fd()) };
        if duplicate < 0 {
            return Err(failed("candidate directory descriptor duplication failed"));
        }
        let stream = unsafe { fdopendir(duplicate) };
        if stream.is_null() {
            unsafe { drop(File::from_raw_fd(duplicate)) };
            return Err(failed("candidate directory stream open failed"));
        }
        let mut names = Vec::new();
        let read_result = loop {
            if let Some(authorize) = authorize.as_deref_mut() {
                if let Err(error) = authorize(path, DescriptorReadOperation::ReadDirectory) {
                    break Err(error);
                }
            }
            // `errno` is reset immediately before `readdir`, never earlier:
            // `authorize` above performs its own G6/storage work, whose
            // internal syscalls (e.g. a `stat` that legitimately returns
            // ENOENT for an absent lock/WAL file) can leave a stale nonzero
            // `errno` behind. POSIX `readdir` guarantees `errno` reflects
            // its own outcome only when checked without an intervening
            // call; resetting any earlier would misattribute an unrelated
            // side effect from `authorize` as a directory-read failure on
            // a legitimate end-of-stream.
            unsafe { *__error() = 0 };
            let entry = unsafe { readdir(stream) };
            if entry.is_null() {
                let error = unsafe { *__error() };
                break if error == 0 {
                    Ok(())
                } else {
                    Err(failed("candidate directory entry read failed"))
                };
            }
            let entry = unsafe { &*entry };
            let length = usize::from(entry.name_length);
            if length > entry.name.len() {
                break Err(failed("candidate directory entry name exceeded bound"));
            }
            let bytes =
                unsafe { std::slice::from_raw_parts(entry.name.as_ptr().cast::<u8>(), length) };
            if bytes != b"." && bytes != b".." {
                names.push(OsString::from_vec(bytes.to_vec()));
            }
        };
        let close_result = unsafe { closedir(stream) };
        read_result?;
        if close_result != 0 {
            return Err(failed("candidate directory stream close failed"));
        }
        Ok(names)
    }

    fn read_authorized(
        file: &File,
        path: &[u8],
        authorize: &mut ReadAuthorizer<'_>,
        limit: u64,
        output: &mut Vec<u8>,
    ) -> Result<(), evaluation::EffectError> {
        use std::io::Read;
        let mut reader = file.take(limit);
        let mut buffer = [0u8; 16 * 1024];
        loop {
            authorize(path, DescriptorReadOperation::ReadFile)?;
            let count = reader
                .read(&mut buffer)
                .map_err(|_| failed("candidate snapshot file read failed"))?;
            if count == 0 {
                return Ok(());
            }
            output.extend_from_slice(&buffer[..count]);
            if output.len() as u64 >= limit {
                return Ok(());
            }
        }
    }

    fn validate_files(files: &[CandidateFile]) -> Result<(), evaluation::EffectError> {
        if files.len() > MAX_CANDIDATE_ENTRIES {
            return Err(failed("candidate snapshot exceeded entry bound"));
        }
        let mut total = 0u64;
        let mut previous: Option<&[u8]> = None;
        let mut normalized = std::collections::BTreeSet::new();
        for file in files {
            if file.mode != 0o100644 && file.mode != 0o100755 {
                return Err(failed("candidate snapshot mode is unsupported"));
            }
            validate_name_path(&file.path)?;
            if previous.is_some_and(|value| value >= file.path.as_slice()) {
                return Err(failed("candidate snapshot paths are ambiguous"));
            }
            previous = Some(&file.path);
            let key = file
                .path
                .iter()
                .map(u8::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if !normalized.insert(key) {
                return Err(failed("candidate snapshot paths collide"));
            }
            total = total
                .checked_add(file.bytes.len() as u64)
                .ok_or_else(|| failed("candidate snapshot exceeded byte bound"))?;
            if total > MAX_CANDIDATE_BLOB_BYTES {
                return Err(failed("candidate snapshot exceeded byte bound"));
            }
        }
        Ok(())
    }

    fn validate_name_path(path: &[u8]) -> Result<(), evaluation::EffectError> {
        if path.is_empty() || path.len() > 4096 || !path.is_ascii() {
            return Err(failed("candidate snapshot path is unsupported"));
        }
        for part in path.split(|byte| *byte == b'/') {
            validate_name(part)?;
        }
        Ok(())
    }

    fn validate_name(name: &[u8]) -> Result<(), evaluation::EffectError> {
        if name.is_empty()
            || name == b"."
            || name == b".."
            || name.eq_ignore_ascii_case(b".git")
            || !name.is_ascii()
            || name
                .iter()
                .any(|byte| *byte == b'\\' || *byte < 0x20 || *byte == 0x7f)
        {
            return Err(failed("candidate snapshot path is unsupported"));
        }
        Ok(())
    }

    fn open_directory(path: &Path) -> Result<File, evaluation::EffectError> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| failed("snapshot path contains NUL"))?;
        let fd = unsafe {
            openat(
                AT_FDCWD,
                path.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(failed("snapshot directory open failed"));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open_child_directory(parent: &File, name: &str) -> Result<File, evaluation::EffectError> {
        let name = component(name)?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(failed("snapshot path contains an unsupported entry"));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn component(value: &str) -> Result<CString, evaluation::EffectError> {
        validate_name(value.as_bytes())?;
        CString::new(value).map_err(|_| failed("snapshot path contains NUL"))
    }

    fn validate_component(value: &str) -> Result<(), evaluation::EffectError> {
        validate_name(value.as_bytes())
    }

    fn failed(reason: &'static str) -> evaluation::EffectError {
        evaluation::EffectError::CaptureFailed(reason)
    }
}

#[cfg(target_os = "macos")]
pub fn materialize_candidate_snapshot(
    parent: &Path,
    name: &str,
    files: &[CandidateFile],
) -> Result<PathBuf, evaluation::EffectError> {
    macos::materialize(parent, name, files)
}

#[cfg(not(target_os = "macos"))]
pub fn materialize_candidate_snapshot(
    _parent: &Path,
    _name: &str,
    _files: &[CandidateFile],
) -> Result<PathBuf, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe candidate snapshots are unsupported on this platform",
    ))
}

#[cfg(target_os = "macos")]
pub fn read_candidate_snapshot(root: &Path) -> Result<CandidateSnapshot, evaluation::EffectError> {
    macos::read(root)
}

#[cfg(target_os = "macos")]
pub fn remove_candidate_snapshot(parent: &Path, name: &str) -> Result<(), evaluation::EffectError> {
    macos::remove(parent, name)
}

#[cfg(not(target_os = "macos"))]
pub fn remove_candidate_snapshot(
    _parent: &Path,
    _name: &str,
) -> Result<(), evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe candidate snapshot removal is unsupported on this platform",
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn read_candidate_snapshot(_root: &Path) -> Result<CandidateSnapshot, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe candidate snapshots are unsupported on this platform",
    ))
}

#[cfg(target_os = "macos")]
pub fn scan_live_worktree(
    root: &Path,
    authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<CandidateSnapshot, evaluation::EffectError> {
    macos::scan_live_worktree(root, authorize)
}

#[cfg(target_os = "macos")]
pub fn read_worktree_git_pointer(
    root: &Path,
    authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Vec<u8>, evaluation::EffectError> {
    macos::read_worktree_git_pointer(root, authorize)
}

#[cfg(not(target_os = "macos"))]
pub fn read_worktree_git_pointer(
    _root: &Path,
    _authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Vec<u8>, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe worktree pointer reads are unsupported",
    ))
}

#[cfg(target_os = "macos")]
pub fn read_linked_metadata_file(
    common_dir: &Path,
    relative_path: &[u8],
    authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Vec<u8>, evaluation::EffectError> {
    macos::read_linked_metadata_file(common_dir, relative_path, authorize)
}

#[cfg(target_os = "macos")]
pub fn read_linked_metadata_file_if_present(
    common_dir: &Path,
    relative_path: &[u8],
    authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Option<Vec<u8>>, evaluation::EffectError> {
    macos::read_linked_metadata_file_if_present(common_dir, relative_path, authorize)
}

#[cfg(not(target_os = "macos"))]
pub fn read_linked_metadata_file(
    _common_dir: &Path,
    _relative_path: &[u8],
    _authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Vec<u8>, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe linked metadata reads are unsupported",
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn read_linked_metadata_file_if_present(
    _common_dir: &Path,
    _relative_path: &[u8],
    _authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<Option<Vec<u8>>, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe linked metadata reads are unsupported",
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn scan_live_worktree(
    _root: &Path,
    _authorize: &mut impl FnMut(&[u8], DescriptorReadOperation) -> Result<(), evaluation::EffectError>,
) -> Result<CandidateSnapshot, evaluation::EffectError> {
    Err(evaluation::EffectError::Unsupported(
        "descriptor-safe candidate worktree scans are unsupported on this platform",
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn materialized_snapshot_round_trips_sealed_bytes_and_detects_write_permission() {
        let parent = std::env::temp_dir().join(format!(
            "runlith-candidate-snapshot-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&parent).expect("create isolated snapshot test root");
        let files = vec![
            CandidateFile {
                path: b"src/nested/input.txt".to_vec(),
                mode: 0o100644,
                git_oid: None,
                bytes: b"verified candidate bytes".to_vec(),
            },
            CandidateFile {
                path: b"tools/check.sh".to_vec(),
                mode: 0o100755,
                git_oid: None,
                bytes: b"#!/bin/sh\n".to_vec(),
            },
        ];
        let root = materialize_candidate_snapshot(&parent, "candidate-test-1", &files)
            .expect("materialize candidate snapshot");
        let observed = read_candidate_snapshot(&root).expect("read sealed snapshot");
        assert_eq!(observed.files, files);
        assert_eq!(
            observed.directories,
            vec![b"src".to_vec(), b"src/nested".to_vec(), b"tools".to_vec()]
        );

        let target = root.join("src/nested/input.txt");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
            .expect("tamper snapshot permissions");
        assert!(read_candidate_snapshot(&root).is_err());

        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444))
            .expect("restore snapshot permissions for cleanup");
        std::fs::set_permissions(
            root.join("src/nested"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("enable nested cleanup");
        std::fs::set_permissions(root.join("src"), std::fs::Permissions::from_mode(0o755))
            .expect("enable source cleanup");
        std::fs::set_permissions(root.join("tools"), std::fs::Permissions::from_mode(0o755))
            .expect("enable tools cleanup");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))
            .expect("enable snapshot cleanup");
        std::fs::remove_dir_all(parent).expect("remove isolated snapshot test root");
    }

    #[test]
    fn snapshot_removal_is_descriptor_relative_and_refuses_symlink_roots() {
        use std::os::unix::fs::symlink;

        let parent = std::env::temp_dir().join(format!(
            "runlith-candidate-snapshot-remove-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&parent).expect("create isolated snapshot parent");
        let files = vec![CandidateFile {
            path: b"nested/input.txt".to_vec(),
            mode: 0o100644,
            git_oid: None,
            bytes: b"sealed".to_vec(),
        }];
        let root = materialize_candidate_snapshot(&parent, "candidate-remove-ok", &files)
            .expect("materialize snapshot");
        remove_candidate_snapshot(&parent, "candidate-remove-ok")
            .expect("remove snapshot through descriptor walk");
        assert!(!root.exists());

        let outside = parent.join("outside");
        std::fs::create_dir(&outside).expect("outside directory");
        std::fs::write(outside.join("keep"), b"preserve").expect("outside sentinel");
        let snapshots = parent.join("candidate-snapshots");
        assert!(
            snapshots.is_dir(),
            "snapshot container remains after removal"
        );
        symlink(&outside, snapshots.join("candidate-remove-link")).expect("snapshot symlink");
        assert!(remove_candidate_snapshot(&parent, "candidate-remove-link").is_err());
        assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"preserve");
        std::fs::remove_dir_all(parent).expect("remove isolated snapshot parent");
    }

    #[test]
    fn live_worktree_scan_captures_only_regular_inputs_and_root_git_pointer() {
        let root = std::env::temp_dir().join(format!(
            "runlith-candidate-worktree-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("create isolated worktree root");
        let root = root.canonicalize().expect("canonicalize worktree root");
        std::fs::write(root.join(".git"), b"gitdir: .git/worktrees/task\n")
            .expect("create linked worktree pointer fixture");
        std::fs::create_dir_all(root.join("src")).expect("create source directory");
        std::fs::write(root.join("src/input.txt"), b"candidate input")
            .expect("create source fixture");

        let mut authorized = Vec::new();
        let mut allow = |path: &[u8],
                         operation: DescriptorReadOperation|
         -> Result<(), evaluation::EffectError> {
            authorized.push((path.to_vec(), operation));
            Ok(())
        };
        let snapshot = scan_live_worktree(&root, &mut allow).expect("scan linked worktree");
        assert!(authorized
            .iter()
            .any(|(_, operation)| { *operation == DescriptorReadOperation::ReadDirectory }));
        assert!(authorized
            .iter()
            .any(|(_, operation)| *operation == DescriptorReadOperation::ReadFile));
        assert_eq!(
            snapshot.git_pointer.as_deref(),
            Some(&b"gitdir: .git/worktrees/task\n"[..])
        );
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, b"src/input.txt");
        assert_eq!(snapshot.files[0].bytes, b"candidate input");
        assert_eq!(snapshot.directories, vec![b"src".to_vec()]);

        std::os::unix::fs::symlink("src/input.txt", root.join("extra-link"))
            .expect("create unsupported symlink fixture");
        let mut deny = |_: &[u8], _: DescriptorReadOperation| {
            Err(evaluation::EffectError::CaptureFailed(
                "descriptor authority denied",
            ))
        };
        assert!(scan_live_worktree(&root, &mut deny).is_err());
        std::fs::remove_dir_all(root).expect("remove isolated worktree root");
    }

    #[test]
    fn linked_git_metadata_reads_are_descriptor_scoped_and_detect_pointer_changes() {
        let base = std::env::temp_dir().join(format!(
            "runlith-linked-metadata-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&base).expect("create linked metadata test root");
        let base = base.canonicalize().expect("canonicalize test root");
        let root = base.join("worktree");
        let common = base.join("common");
        let gitdir = common.join("worktrees/task");
        std::fs::create_dir_all(&root).expect("create worktree");
        std::fs::create_dir_all(&gitdir).expect("create linked metadata");
        let pointer = format!("gitdir: {}\n", gitdir.display());
        std::fs::write(root.join(".git"), pointer.as_bytes()).expect("write worktree pointer");
        std::fs::write(
            gitdir.join("gitdir"),
            root.join(".git").to_string_lossy().as_bytes(),
        )
        .expect("write linked back-reference");
        std::fs::write(gitdir.join("commondir"), b"../..\n").expect("write common reference");

        let mut allow = |_: &[u8], _: DescriptorReadOperation| Ok(());
        let before = read_worktree_git_pointer(&root, &mut allow).expect("read initial pointer");
        let backref = read_linked_metadata_file(&common, b"worktrees/task/gitdir", &mut allow)
            .expect("descriptor-read linked back-reference");
        assert_eq!(
            String::from_utf8_lossy(&backref),
            root.join(".git").to_string_lossy()
        );

        std::fs::write(root.join(".git"), b"gitdir: changed\n").expect("tamper pointer");
        let after = read_worktree_git_pointer(&root, &mut allow).expect("read changed pointer");
        assert_ne!(
            before, after,
            "paired observations must detect pointer mutation"
        );

        std::fs::remove_file(gitdir.join("commondir")).expect("remove regular metadata file");
        std::os::unix::fs::symlink("../../outside", gitdir.join("commondir"))
            .expect("replace metadata with symlink");
        assert!(
            read_linked_metadata_file(&common, b"worktrees/task/commondir", &mut allow,).is_err()
        );
        std::fs::remove_dir_all(base).expect("remove linked metadata test tree");
    }
}
