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
