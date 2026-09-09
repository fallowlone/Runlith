//! Windows local transport: Named Pipe byte streams with explicit security.
//!
//! Compiled only on Windows. Pipe identity carries the current-user SID
//! (never username, email, or display name); the DACL grants pipe access
//! only to the current user SID and LOCAL SYSTEM; remote clients are
//! rejected; byte mode keeps the same explicit length framing as Unix; and
//! the first server instance owns the pipe so a second daemon fails
//! observably. All `unsafe` here is small, platform-local, and documented.
//!
//! Peer authentication beyond this per-user endpoint boundary remains
//! PRE-G2 by explicit classification: `local != trusted`.

use std::io;
use std::ptr;

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions,
};
#[cfg(test)]
use windows_sys::core::BOOL;
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
#[cfg(test)]
use windows_sys::Win32::Security::{
    AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorDacl, ACL_SIZE_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows_sys::Win32::Security::{
    AddAccessAllowedAceEx, CreateWellKnownSid, GetLengthSid, GetTokenInformation, InitializeAcl,
    InitializeSecurityDescriptor, SetSecurityDescriptorDacl, TokenUser, WinLocalSystemSid,
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, PSID, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::framing::FramedTransport;
use crate::{TransportError, TransportResult};

/// Canonical pipe-name prefix; the current-user SID string is appended.
pub const PIPE_NAME_PREFIX: &str = r"\\.\pipe\ai-engineering-os-agentd-";

/// `GENERIC_READ | GENERIC_WRITE` as the pipe ACE access mask.
/// (Named locally to avoid pulling `Win32_Storage_FileSystem` for two
/// stable constants; values per Win32 API.)
const PIPE_ACCESS_MASK: u32 = 0x8000_0000 | 0x4000_0000;

/// `SECURITY_DESCRIPTOR_REVISION` per Win32 API (not exported by
/// `windows-sys` 0.61, so pinned here with its provenance documented).
const SECURITY_DESCRIPTOR_REVISION_VALUE: u32 = 1;

/// `ACCESS_ALLOWED_ACE_TYPE` per Win32 API (lives in
/// `Win32_System_SystemServices`; pinned here to keep the Windows feature
/// set minimal).
#[cfg(test)]
const ACCESS_ALLOWED_ACE_TYPE_VALUE: u8 = 0;

/// Bounded wide-string read for OS-produced SID text.
const MAX_SID_TEXT_LEN: usize = 512;

/// Builds the canonical pipe name for an explicit SID string.
/// Pure function, independently testable without OS calls.
#[must_use]
pub fn pipe_name_for_sid(sid: &str) -> String {
    format!("{PIPE_NAME_PREFIX}{sid}")
}

/// Environment override for the full pipe name (test seam).
///
/// When `AI_ENGINEERING_OS_PIPE_NAME` is set and non-empty, pipe
/// resolution uses it verbatim instead of the SID-suffixed canonical
/// name, so real-process tests run against a test-owned pipe. Production
/// never sets this variable.
pub const PIPE_NAME_OVERRIDE_ENV: &str = "AI_ENGINEERING_OS_PIPE_NAME";

/// Resolves the canonical pipe identity for the current Windows user.
pub fn pipe_name() -> TransportResult<String> {
    if let Some(name) = std::env::var_os(PIPE_NAME_OVERRIDE_ENV).filter(|v| !v.is_empty()) {
        return Ok(name.to_string_lossy().into_owned());
    }
    Ok(pipe_name_for_sid(&current_user_sid()?))
}

/// Returns the current process user SID as a string (`S-...`).
pub fn current_user_sid() -> TransportResult<String> {
    let sid_bytes = current_user_token_sid_bytes()?;
    // Safety: `sid_bytes` is a complete SID produced by the OS token query
    // below; `ConvertSidToStringSidW` allocates the output string, which we
    // read bounded and free exactly once on every path.
    unsafe {
        let mut wide: PWSTR = ptr::null_mut();
        if ConvertSidToStringSidW(sid_bytes.as_ptr() as PSID, &mut wide) == 0 {
            return Err(env_error("ConvertSidToStringSidW"));
        }
        if wide.is_null() {
            return Err(TransportError::InvalidEndpoint(
                "SID string conversion returned null".to_owned(),
            ));
        }
        let mut len = 0usize;
        while len < MAX_SID_TEXT_LEN && *wide.add(len) != 0 {
            len += 1;
        }
        if len == MAX_SID_TEXT_LEN {
            LocalFree(wide.cast::<core::ffi::c_void>());
            return Err(TransportError::InvalidEndpoint(
                "SID string is not NUL-terminated within bounds".to_owned(),
            ));
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(wide, len));
        LocalFree(wide.cast::<core::ffi::c_void>());
        Ok(text)
    }
}

/// Returns the current process user SID in binary form.
pub(crate) fn daemon_user_sid() -> TransportResult<Vec<u8>> {
    current_user_token_sid_bytes()
}

/// Returns the current process user SID in binary form.
fn current_user_token_sid_bytes() -> TransportResult<Vec<u8>> {
    // Safety: `OpenProcessToken` hands us a real token handle, closed on
    // every path; the two-phase `GetTokenInformation` sizes the buffer.
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(env_error("OpenProcessToken"));
        }
        let result = token_sid_bytes(token);
        CloseHandle(token);
        result
    }
}

/// Reads the user SID out of an open process or thread token.
pub(crate) unsafe fn token_sid_bytes(token: HANDLE) -> TransportResult<Vec<u8>> {
    // Safety invariants: `token` is a live process-token handle owned by the
    // caller; the first call reports the required length; the second fills a
    // buffer of exactly that size, so the embedded `TOKEN_USER` and its SID
    // are fully inside `buffer`.
    let mut needed: u32 = 0;
    GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
    if needed == 0 || needed > 65_536 {
        return Err(env_error("GetTokenInformation size probe"));
    }
    let mut buffer = vec![0u8; needed as usize];
    if GetTokenInformation(
        token,
        TokenUser,
        buffer.as_mut_ptr().cast::<core::ffi::c_void>(),
        needed,
        &mut needed,
    ) == 0
    {
        return Err(env_error("GetTokenInformation"));
    }
    let user: TOKEN_USER = ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>());
    if user.User.Sid.is_null() {
        return Err(TransportError::InvalidEndpoint(
            "token holds no user SID".to_owned(),
        ));
    }
    let sid_len = GetLengthSid(user.User.Sid) as usize;
    if sid_len == 0 || sid_len > buffer.len() {
        return Err(TransportError::InvalidEndpoint(
            "token user SID has an implausible length".to_owned(),
        ));
    }
    let bytes = std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_len);
    Ok(bytes.to_vec())
}

/// Returns the LOCAL SYSTEM SID in binary form.
fn well_known_system_sid() -> TransportResult<Vec<u8>> {
    let mut buffer = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut len = buffer.len() as u32;
    // Safety: `buffer` is `SECURITY_MAX_SID_SIZE` bytes, the documented
    // maximum SID length; `len` is an in/out length the OS updates.
    unsafe {
        if CreateWellKnownSid(
            WinLocalSystemSid,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast::<core::ffi::c_void>(),
            &mut len,
        ) == 0
        {
            return Err(env_error("CreateWellKnownSid(WinLocalSystemSid)"));
        }
    }
    buffer.truncate(len as usize);
    Ok(buffer)
}

fn env_error(context: &str) -> TransportError {
    TransportError::InvalidEndpoint(format!("{context}: {}", io::Error::last_os_error()))
}

/// Explicit pipe security: DACL granting the required pipe access only to
/// the current user SID and LOCAL SYSTEM. Never Everyone/Anonymous.
///
/// Owns the ACL and descriptor buffers. The `SECURITY_ATTRIBUTES` wrapper
/// is built stack-local per OS call (see
/// [`with_attributes_mut`](Self::with_attributes_mut)): the OS copies the
/// descriptor synchronously during pipe creation, so no raw pointer is ever
/// stored.
pub struct PipeSecurityAttributes {
    // Safety/lifetime: owned backing store for the DACL referenced by
    // `descriptor` via `SetSecurityDescriptorDacl`. The OS reads it during
    // pipe creation; Rust never reads it again, so it looks dead to the
    // compiler but must stay owned to keep the descriptor/ACL pointers valid.
    #[allow(dead_code)]
    acl: Vec<u8>,
    descriptor: SECURITY_DESCRIPTOR,
}

impl PipeSecurityAttributes {
    /// Builds the user+SYSTEM-only security profile.
    pub fn user_and_system_only() -> TransportResult<Self> {
        let user_sid = current_user_token_sid_bytes()?;
        let system_sid = well_known_system_sid()?;
        // Each ACE costs its header + mask + SID bytes; SidStart contributes
        // a 4-byte placeholder already counted in the struct size.
        let ace_len = |sid_len: u32| {
            (std::mem::size_of::<ACCESS_ALLOWED_ACE>() as u32)
                .saturating_add(sid_len)
                .saturating_sub(4)
        };
        let acl_len = (std::mem::size_of::<ACL>() as u32)
            .saturating_add(ace_len(user_sid.len() as u32))
            .saturating_add(ace_len(system_sid.len() as u32));
        let mut acl = vec![0u8; acl_len as usize];
        // Safety: `acl` is exactly `acl_len` bytes; both SIDs are complete
        // OS-produced SIDs; every pointer stays inside owned buffers for the
        // whole block, and nothing escapes except owned values.
        unsafe {
            if InitializeAcl(acl.as_mut_ptr().cast::<ACL>(), acl_len, ACL_REVISION) == 0 {
                return Err(env_error("InitializeAcl"));
            }
            for sid in [&user_sid, &system_sid] {
                if AddAccessAllowedAceEx(
                    acl.as_mut_ptr().cast::<ACL>(),
                    ACL_REVISION,
                    0, // no inheritance flags: named pipes are not containers
                    PIPE_ACCESS_MASK,
                    sid.as_ptr() as PSID,
                ) == 0
                {
                    return Err(env_error("AddAccessAllowedAceEx"));
                }
            }
            let mut descriptor: SECURITY_DESCRIPTOR = std::mem::zeroed();
            if InitializeSecurityDescriptor(
                ptr::addr_of_mut!(descriptor).cast::<core::ffi::c_void>(),
                SECURITY_DESCRIPTOR_REVISION_VALUE,
            ) == 0
            {
                return Err(env_error("InitializeSecurityDescriptor"));
            }
            if SetSecurityDescriptorDacl(
                ptr::addr_of_mut!(descriptor).cast::<core::ffi::c_void>(),
                1, // DACL present (never a null-DACL "allow all" descriptor)
                acl.as_ptr().cast::<ACL>(),
                0, // not defaulted
            ) == 0
            {
                return Err(env_error("SetSecurityDescriptorDacl"));
            }
            Ok(Self { acl, descriptor })
        }
    }

    /// Runs `f` with a valid `SECURITY_ATTRIBUTES` pointer. The attributes
    /// struct is stack-local to the call; the OS copies the descriptor
    /// before returning, so the pointer never outlives this call.
    fn with_attributes_mut<R>(&mut self, f: impl FnOnce(*mut core::ffi::c_void) -> R) -> R {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::addr_of_mut!(self.descriptor).cast::<core::ffi::c_void>(),
            bInheritHandle: 0,
        };
        f(ptr::addr_of_mut!(attributes).cast::<core::ffi::c_void>())
    }

    #[cfg(test)]
    fn descriptor_ptr(&self) -> PSECURITY_DESCRIPTOR {
        ptr::addr_of!(self.descriptor) as PSECURITY_DESCRIPTOR
    }
}

/// Creates one pipe server instance with the secure profile: byte mode,
/// remote clients rejected, bounded instances, explicit DACL.
/// `first_instance` selects the singleton first-instance flag.
fn create_server(name: &str, first_instance: bool) -> TransportResult<NamedPipeServer> {
    let mut security = PipeSecurityAttributes::user_and_system_only()?;
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .reject_remote_clients(true)
        .max_instances(crate::MAX_CONTROL_CLIENTS)
        .first_pipe_instance(first_instance);
    // Safety: `security` outlives the call; `with_attributes_mut` provides a
    // valid `SECURITY_ATTRIBUTES` for exactly the duration of the OS call,
    // which is what `create_with_security_attributes_raw` requires.
    security
        .with_attributes_mut(|attrs| unsafe {
            options.create_with_security_attributes_raw(name, attrs)
        })
        .map_err(|error| map_create_error(name, first_instance, error))
}

fn map_create_error(name: &str, first_instance: bool, error: io::Error) -> TransportError {
    if first_instance {
        let owned_by_kind = matches!(
            error.kind(),
            io::ErrorKind::AlreadyExists
                | io::ErrorKind::PermissionDenied
                | io::ErrorKind::AddrInUse
        );
        let owned_by_code = matches!(
            error.raw_os_error(),
            Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_PIPE_BUSY as i32
        );
        if owned_by_kind || owned_by_code {
            // A second daemon lost the singleton race: observable as
            // endpoint-owned so TASK-0020 reports AlreadyRunning.
            return TransportError::EndpointOwned {
                endpoint: name.to_owned(),
            };
        }
    }
    if error.kind() == io::ErrorKind::PermissionDenied {
        return TransportError::EndpointSecurity(format!("cannot create pipe {name}: {error}"));
    }
    TransportError::Io(error)
}

/// Server side of the canonical pipe: first-instance bind plus the
/// create-listen-handoff-create-next accept pattern within the
/// [`MAX_CONTROL_CLIENTS`](crate::MAX_CONTROL_CLIENTS) policy. No
/// handshake/session tasks.
///
/// Creation requires a Tokio runtime context with I/O enabled.
pub struct PipeListener {
    name: String,
    pending: Option<NamedPipeServer>,
}

impl PipeListener {
    /// Binds the canonical pipe as the first instance: success owns the
    /// daemon endpoint; failure maps to [`TransportError::EndpointOwned`]
    /// when another daemon already owns it.
    pub fn bind_first() -> TransportResult<Self> {
        let name = pipe_name()?;
        let first = create_server(&name, true)?;
        Ok(Self {
            name,
            pending: Some(first),
        })
    }

    /// Canonical pipe name under management.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Accepts one client: keeps a listening instance available while
    /// handing off the connected one. Never relies on a single permanent
    /// connected instance.
    pub async fn accept(&mut self) -> TransportResult<NamedPipeServer> {
        let server = self.pending.take().ok_or_else(|| {
            TransportError::InvalidEndpoint("pipe listener has no pending instance".to_owned())
        })?;
        let next = create_server(&self.name, false)?;
        self.pending = Some(next);
        server.connect().await.map_err(TransportError::from)?;
        Ok(server)
    }
}

/// Connects a local client stream. Byte mode is requested explicitly;
/// lifecycle (spawn/retry) belongs to TASK-0020.
pub fn connect_client(name: &str) -> TransportResult<NamedPipeClient> {
    ClientOptions::new()
        .pipe_mode(PipeMode::Byte)
        .open(name)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => TransportError::EndpointUnavailable {
                endpoint: name.to_owned(),
            },
            io::ErrorKind::PermissionDenied => {
                TransportError::EndpointSecurity(format!("cannot open pipe {name}: {error}"))
            }
            _ => TransportError::Io(error),
        })
}

/// Wraps a connected pipe stream with G1 framing.
pub fn framed_server(server: NamedPipeServer) -> FramedTransport<NamedPipeServer> {
    FramedTransport::new(server)
}

/// Wraps a connected pipe client with G1 framing.
pub fn framed_client(client: NamedPipeClient) -> FramedTransport<NamedPipeClient> {
    FramedTransport::new(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_pipe_name(suite: &str) -> String {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!(
            "{}{}-test-{}-{}-{}",
            PIPE_NAME_PREFIX,
            current_user_sid().expect("sid"),
            std::process::id(),
            suite,
            id
        )
    }

    #[test]
    fn pipe_name_for_sid_uses_sid_identity() {
        let name = pipe_name_for_sid("S-1-5-21-1-2-3-4");
        assert_eq!(name, r"\\.\pipe\ai-engineering-os-agentd-S-1-5-21-1-2-3-4");
    }

    #[test]
    fn pipe_name_carries_current_user_sid() {
        let name = pipe_name().expect("pipe name");
        assert!(name.starts_with(PIPE_NAME_PREFIX), "prefix: {name}");
        let sid = &name[PIPE_NAME_PREFIX.len()..];
        assert!(sid.starts_with("S-1-"), "SID string, got {sid}");
        assert!(
            !sid.contains(['\\', '/']),
            "SID must be a single path element"
        );
    }

    #[test]
    fn security_descriptor_grants_only_user_and_system() {
        let security = PipeSecurityAttributes::user_and_system_only().expect("attrs");
        let user_sid = current_user_token_sid_bytes().expect("user sid");
        let system_sid = well_known_system_sid().expect("system sid");
        // Safety: read-back of our own in-memory descriptor/ACL; every
        // pointer is derived from owned buffers sized by the OS.
        unsafe {
            let mut present: BOOL = 0;
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut defaulted: BOOL = 0;
            assert_ne!(
                GetSecurityDescriptorDacl(
                    security.descriptor_ptr(),
                    &mut present,
                    &mut dacl,
                    &mut defaulted
                ),
                0,
                "DACL must be present"
            );
            assert_ne!(
                present, 0,
                "null-DACL (allow-all) descriptors are forbidden"
            );
            assert!(!dacl.is_null());
            let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
            assert_ne!(
                GetAclInformation(
                    dacl,
                    ptr::addr_of_mut!(info).cast::<core::ffi::c_void>(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                ),
                0
            );
            assert_eq!(info.AceCount, 2, "exactly user + SYSTEM");
            let mut saw_user = false;
            let mut saw_system = false;
            for index in 0..info.AceCount {
                let mut ace: *mut core::ffi::c_void = ptr::null_mut();
                assert_ne!(GetAce(dacl, index, &mut ace), 0);
                let ace = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
                assert_eq!(ace.Header.AceType, ACCESS_ALLOWED_ACE_TYPE_VALUE);
                assert_eq!(ace.Mask, PIPE_ACCESS_MASK);
                let sid_ptr = ptr::addr_of!(ace.SidStart).cast::<u8>();
                let sid_len = GetLengthSid(sid_ptr as PSID) as usize;
                let sid = std::slice::from_raw_parts(sid_ptr, sid_len);
                if sid == &user_sid[..] {
                    saw_user = true;
                } else if sid == &system_sid[..] {
                    saw_system = true;
                } else {
                    panic!("ACE {index} grants an unexpected SID");
                }
            }
            assert!(
                saw_user && saw_system,
                "both user and SYSTEM must be granted"
            );
        }
    }

    #[tokio::test]
    async fn second_first_instance_owner_fails_observably() {
        let name = test_pipe_name("singleton");
        let _first = create_server(&name, true).expect("first owner binds");
        match create_server(&name, true) {
            Err(TransportError::EndpointOwned { endpoint }) => assert_eq!(endpoint, name),
            other => panic!("expected EndpointOwned, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pipe_client_server_framed_round_trip() {
        let name = test_pipe_name("roundtrip");
        let server = create_server(&name, false).expect("server");
        let mut client = framed_client(connect_client(&name).expect("client"));
        server.connect().await.expect("server connect");
        let mut server = framed_server(server);

        client
            .write_frame(Bytes::from_static(b"hello-daemon"))
            .await
            .expect("client write");
        let got = server.read_frame().await.expect("server read");
        assert_eq!(&got[..], b"hello-daemon");

        server
            .write_frame(Bytes::from_static(b"hello-client"))
            .await
            .expect("server write");
        let back = client.read_frame().await.expect("client read");
        assert_eq!(&back[..], b"hello-client");
    }

    #[tokio::test]
    async fn pipe_listener_accept_hands_off_and_keeps_listening() {
        let name = test_pipe_name("accept");
        let first = create_server(&name, true).expect("bind first");
        let mut listener = PipeListener {
            name: name.clone(),
            pending: Some(first),
        };
        let connect_task = tokio::spawn({
            let name = name.clone();
            async move { connect_client(&name).expect("client") }
        });
        let server = listener.accept().await.expect("accept");
        let _client = connect_task.await.expect("join");
        assert!(
            listener.pending.is_some(),
            "listening instance must be renewed"
        );
        drop(server);
    }

    #[test]
    fn remote_rejection_is_configured_in_the_single_constructor() {
        // No stable Win32 read-back API exposes PIPE_REJECT_REMOTE_CLIENTS,
        // so this pins the invariant at the construction site: every server
        // instance in this crate is built by `create_server`, which sets
        // `reject_remote_clients(true)` explicitly alongside byte mode and
        // the explicit DACL. The singleton + round-trip tests above execute
        // that constructor.
        let name = test_pipe_name("ctor");
        let _server = create_server(&name, false).expect("secure constructor binds");
    }
}
