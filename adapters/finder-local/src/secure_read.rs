use std::fs::File;
use std::io;
#[cfg(unix)]
use std::path::Component;
use std::path::Path;

#[cfg(unix)]
pub(crate) fn open_beneath(root: &Path, relative: &Path) -> io::Result<File> {
    use rustix::fs::{open, openat, Mode, OFlags};
    let directory_flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let mut directory = open(root, directory_flags, Mode::empty())?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "non-relative file path",
            ));
        };
        if components.peek().is_none() {
            let file = File::from(openat(
                &directory,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )?);
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            return Ok(file);
        }
        directory = openat(&directory, name, directory_flags, Mode::empty())?;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "empty file path",
    ))
}

#[cfg(not(unix))]
pub(crate) fn open_beneath(_: &Path, _: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-based file reads unavailable",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{IngestedFile, LocalFinder};
    use repo_finder::{FinderError, RepoProvenance, RepoRevisionPort, RepoRoot, Revision};
    use repo_finder_contract::ContractRepo;
    use std::sync::Arc;

    struct RevisionFixture;
    impl RepoRevisionPort for RevisionFixture {
        fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
            Ok(RepoProvenance {
                root: root.clone(),
                revision: Revision::new("fixture").unwrap(),
                dirty: false,
            })
        }
    }

    #[test]
    fn ingest_refuses_a_parent_swapped_to_an_outside_symlink_after_the_walk() {
        let repo = ContractRepo::build();
        let outside = ContractRepo::build();
        let root = repo.root().as_path();
        let parent = root.join("swapped");
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("canary.txt"), b"inside").unwrap();
        std::fs::write(
            outside.root().as_path().join("canary.txt"),
            b"outside-canary",
        )
        .unwrap();
        let finder = LocalFinder::new(Arc::new(RevisionFixture));
        let relative = Path::new("swapped/canary.txt");
        let mut walked = Vec::new();
        finder.walk(root, &mut walked).unwrap();
        assert!(walked.contains(&relative.to_path_buf()));
        let IngestedFile::Content { lines, .. } = finder.ingest_file(root, relative).unwrap()
        else {
            panic!("fixture file skipped");
        };
        assert_eq!(lines, ["inside"]);
        std::fs::rename(&parent, root.join("saved")).unwrap();
        std::os::unix::fs::symlink(outside.root().as_path(), &parent).unwrap();
        assert!(finder.ingest_file(root, relative).is_err());
        assert!(open_beneath(root, Path::new("../canary.txt")).is_err());
    }
}
