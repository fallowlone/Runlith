use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::process::Stdio;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

mod macos;
#[cfg(test)]
mod tests;

const HEADER: &[u8] = b"runlith-candidate-launch/v1\n";
const LIFETIME: Duration = Duration::from_secs(30);
const MAX_ARGS: usize = 6;
const MAX_ARG_BYTES: usize = 4096;
const MAX_INPUT: u64 = evaluation::MAX_CANDIDATE_BLOB_BYTES + 64 * 1024 * 1024;

#[cfg(test)]
pub(crate) const OUTPUT_MARKER: &[u8] = b"\nrunlith-candidate-fixture-output/v1\n";

pub(crate) struct Permit {
    channel: UnixStream,
    child: Option<UnixStream>,
    pub(crate) prefix: Vec<u8>,
    expected: [u8; 64],
    deadline: Instant,
}

impl Permit {
    pub(crate) fn issue(args: &[OsString], input: &[u8]) -> io::Result<Self> {
        let args = args
            .iter()
            .map(|arg| arg.to_str().map(str::to_owned).ok_or_else(denied))
            .collect::<io::Result<Vec<_>>>()?;
        if args.is_empty() || args.len() > MAX_ARGS || input.len() as u64 > MAX_INPUT {
            return Err(denied());
        }
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(io::Error::other)?;
        let mut prefix = HEADER.to_vec();
        prefix.extend_from_slice(&nonce);
        prefix.push(args.len() as u8);
        for arg in &args {
            if arg.len() > MAX_ARG_BYTES {
                return Err(denied());
            }
            prefix.extend_from_slice(&(arg.len() as u32).to_be_bytes());
            prefix.extend_from_slice(arg.as_bytes());
        }
        let mut expected = [0; 64];
        expected[..32].copy_from_slice(&nonce);
        expected[32..].copy_from_slice(&binding(&args, input));
        let (channel, child) = UnixStream::pair()?;
        Ok(Self {
            channel,
            child: Some(child),
            prefix,
            expected,
            deadline: Instant::now() + LIFETIME,
        })
    }

    pub(crate) fn take_stdio(&mut self) -> io::Result<Stdio> {
        let fd: OwnedFd = self.child.take().ok_or_else(denied)?.into();
        Ok(Stdio::from(fd))
    }

    pub(crate) fn serve(mut self) -> io::Result<()> {
        let mut request = [0; 64];
        read_before(&mut self.channel, &mut request, self.deadline)?;
        if request != self.expected || Instant::now() >= self.deadline {
            return Err(denied());
        }
        self.channel.set_write_timeout(Some(
            self.deadline.saturating_duration_since(Instant::now()),
        ))?;
        self.channel.write_all(&[1])
    }
}

pub(crate) fn authenticate(expected_args: Option<&[String]>) -> io::Result<(Vec<String>, Vec<u8>)> {
    let fd = std::io::stderr().as_fd().try_clone_to_owned()?;
    let mut channel = UnixStream::from(fd);
    macos::verify_parent(&channel)?;
    let deadline = Instant::now() + LIFETIME;
    let mut stdin = std::io::stdin().lock();
    let mut header = [0; HEADER.len()];
    stdin.read_exact(&mut header)?;
    if header != HEADER {
        return Err(denied());
    }
    let mut nonce = [0; 32];
    stdin.read_exact(&mut nonce)?;
    let mut count = [0];
    stdin.read_exact(&mut count)?;
    if count[0] == 0 || count[0] as usize > MAX_ARGS {
        return Err(denied());
    }
    let mut args = Vec::with_capacity(count[0] as usize);
    for _ in 0..count[0] {
        let mut length = [0; 4];
        stdin.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_ARG_BYTES {
            return Err(denied());
        }
        let mut bytes = vec![0; length];
        stdin.read_exact(&mut bytes)?;
        args.push(String::from_utf8(bytes).map_err(|_| denied())?);
    }
    if expected_args.is_some_and(|expected| args != expected) {
        return Err(denied());
    }
    let mut input = Vec::new();
    stdin.take(MAX_INPUT + 1).read_to_end(&mut input)?;
    if input.len() as u64 > MAX_INPUT || Instant::now() >= deadline {
        return Err(denied());
    }
    let mut request = [0; 64];
    request[..32].copy_from_slice(&nonce);
    request[32..].copy_from_slice(&binding(&args, &input));
    arm(&channel, deadline)?;
    channel.write_all(&request)?;
    read_reply(&mut channel)?;
    Ok((args, input))
}

/// Sets both socket timeouts before the request leaves. macOS rejects
/// `setsockopt` with EINVAL once the peer has closed, and the parent closes
/// right after it replies, so a timeout set after the write can fail on a
/// loaded machine even though the reply is already waiting.
fn arm(channel: &UnixStream, deadline: Instant) -> io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "candidate permit expired",
        ));
    }
    channel.set_write_timeout(Some(remaining))?;
    channel.set_read_timeout(Some(remaining))
}

/// Reads the one-byte grant on a channel already armed by [`arm`].
fn read_reply(channel: &mut UnixStream) -> io::Result<()> {
    let mut reply = [0];
    match channel.read_exact(&mut reply) {
        Ok(()) if reply == [1] => Ok(()),
        Ok(()) => Err(denied()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(denied()),
        Err(error) => Err(error),
    }
}

fn binding(args: &[String], input: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(HEADER);
    hash.update((args.len() as u64).to_be_bytes());
    for arg in args {
        hash.update((arg.len() as u64).to_be_bytes());
        hash.update(arg.as_bytes());
    }
    hash.update((input.len() as u64).to_be_bytes());
    hash.update(input);
    hash.finalize().into()
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "candidate permit expired",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(bytes) {
            Ok(0) => return Err(denied()),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "candidate launch not authorized",
    )
}
