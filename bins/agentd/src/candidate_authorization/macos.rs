use std::ffi::{c_int, c_uint, c_void};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

// XNU bsd/sys/codesign.h and osfmk/kern/cs_blobs.h.
const CS_OPS_STATUS: c_uint = 0;
const CS_OPS_CDHASH: c_uint = 5;
const CS_VALID: u32 = 0x0000_0001;
const CS_INVALID_ALLOWED: u32 = 0x0000_0020;
const CS_DEBUGGED: u32 = 0x1000_0000;

#[repr(C)]
#[derive(Default)]
struct AuditToken {
    words: [u32; 8],
}

unsafe extern "C" {
    fn csops(pid: libc::pid_t, op: c_uint, output: *mut c_void, size: usize) -> c_int;
    fn csops_audittoken(
        pid: libc::pid_t,
        op: c_uint,
        output: *mut c_void,
        size: usize,
        token: *mut AuditToken,
    ) -> c_int;
}

pub(super) fn verify_parent(channel: &UnixStream) -> io::Result<()> {
    let mut peer_pid: libc::pid_t = 0;
    let mut pid_len = size_of::<libc::pid_t>() as libc::socklen_t;
    let mut token = AuditToken::default();
    let mut token_len = size_of::<AuditToken>() as libc::socklen_t;
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: all outputs are initialized, aligned C scalars/arrays with exact
    // writable lengths; the borrowed socket stays alive throughout both calls.
    let admitted = unsafe {
        libc::getpeereid(channel.as_raw_fd(), &mut uid, &mut gid) == 0
            && uid == libc::geteuid()
            && libc::getsockopt(
                channel.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut peer_pid as *mut libc::pid_t).cast(),
                &mut pid_len,
            ) == 0
            && pid_len as usize == size_of::<libc::pid_t>()
            && peer_pid > 1
            && peer_pid == libc::getppid()
            && libc::getsockopt(
                channel.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERTOKEN,
                (&mut token as *mut AuditToken).cast(),
                &mut token_len,
            ) == 0
            && token_len as usize == size_of::<AuditToken>()
    };
    if !admitted {
        return Err(super::denied());
    }
    let mut status = 0u32;
    let mut peer_hash = [0u8; 20];
    let mut own_hash = [0u8; 20];
    // SAFETY: XNU's output sizes are 4 and CS_CDHASH_LEN (20); the audit token
    // came from the kernel and binds parent queries to its process generation.
    let matching = unsafe {
        csops_audittoken(
            peer_pid,
            CS_OPS_STATUS,
            (&mut status as *mut u32).cast(),
            size_of::<u32>(),
            &mut token,
        ) == 0
            && status & (CS_VALID | CS_INVALID_ALLOWED | CS_DEBUGGED) == CS_VALID
            && csops_audittoken(
                peer_pid,
                CS_OPS_CDHASH,
                peer_hash.as_mut_ptr().cast(),
                peer_hash.len(),
                &mut token,
            ) == 0
            && csops(
                0,
                CS_OPS_CDHASH,
                own_hash.as_mut_ptr().cast(),
                own_hash.len(),
            ) == 0
            && peer_hash != [0; 20]
            && peer_hash == own_hash
            && peer_pid == libc::getppid()
    };
    if matching {
        Ok(())
    } else {
        Err(super::denied())
    }
}
