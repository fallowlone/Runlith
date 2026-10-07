//! Pure deterministic negotiation for the G1 bootstrap handshake.
//!
//! No I/O, no state machine, no transport: version selection, client-kind
//! registry checks, and capability intersection. The session layer in
//! [`session`](crate::session) maps faults to `HandshakeRejected` wire
//! responses.

use ipc_contracts::proto::{HandshakeRejected, HandshakeRequest};
use ipc_contracts::{
    IPC_PROTOCOL_VERSION_V0, IPC_PROTOCOL_VERSION_V1, IPC_PROTOCOL_VERSION_V2,
    IPC_PROTOCOL_VERSION_V3, IPC_PROTOCOL_VERSION_V4, IPC_PROTOCOL_VERSION_V5,
    IPC_PROTOCOL_VERSION_V6,
};

use crate::error::{ControlError, ControlResult};

/// G1-recognized client kind for the Tauri host. Descriptive metadata only:
/// never authentication, never authorization.
pub const CLIENT_KIND_TAURI_HOST: &str = "tauri-host";

/// G1-recognized client kind for `agentctl`. Descriptive metadata only:
/// never authentication, never authorization.
pub const CLIENT_KIND_AGENTCTL: &str = "agentctl";

/// Reports whether a `client_kind` is in the exact G1 registry.
#[must_use]
pub fn is_recognized_client_kind(kind: &str) -> bool {
    kind == CLIENT_KIND_TAURI_HOST || kind == CLIENT_KIND_AGENTCTL
}

/// Selects the highest numeric protocol version present in both sets.
/// Ordering and duplicates in either array do not alter the result.
#[must_use]
pub fn select_highest_common_version(client: &[u32], daemon: &[u32]) -> Option<u32> {
    client.iter().filter(|v| daemon.contains(v)).max().copied()
}

/// Intersects requested capabilities with daemon-supported ones, preserving
/// request order without duplicates. Unknown requested capabilities are
/// omitted but never reject the handshake. G1 supports no optional
/// capabilities, so the daemon set is empty and the result is always empty;
/// the function stays generic for future versions. No closed enum.
#[must_use]
pub fn negotiate_capabilities(requested: &[String], supported: &[String]) -> Vec<String> {
    let mut accepted = Vec::new();
    for capability in requested {
        if supported.iter().any(|s| s == capability) && !accepted.iter().any(|a| a == capability) {
            accepted.push(capability.clone());
        }
    }
    accepted
}

/// Reports whether a negotiated session version carries business (G2
/// `task_core`) authority: versions 1 and 2 do. A version-0 session stays
/// bootstrap-only (handshake, ping, shutdown) even though the Protobuf
/// decoder could structurally parse a business frame — decoding is never
/// authority.
#[must_use]
pub fn is_business_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_business_ipc(negotiated_version)
}

/// Reports whether a negotiated session version carries execution (G5
/// `execution_core`) authority: only version 2 does.
#[must_use]
pub fn is_execution_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_execution_ipc(negotiated_version)
}

/// Reports whether a negotiated session carries G6 approval control-plane
/// authority: only version 3 does.
#[must_use]
pub fn is_policy_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_policy_ipc(negotiated_version)
}

#[must_use]
pub fn is_human_decision_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_human_decision_ipc(negotiated_version)
}

#[must_use]
pub fn is_planning_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_planning_ipc(negotiated_version)
}

#[must_use]
pub fn is_task_authoring_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_task_authoring_ipc(negotiated_version)
}

/// Enforces the version gate for one business request: `Ok` on v1+,
/// [`ControlError::UnsupportedProtocol`] otherwise.
pub fn validate_business_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_business_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: IPC_PROTOCOL_VERSION_V1,
        })
    }
}

/// Enforces the version gate for one execution request: `Ok` on v2,
/// [`ControlError::UnsupportedProtocol`] otherwise. A v0/v1 session
/// rejects without executing.
pub fn validate_execution_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_execution_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V2,
        })
    }
}

/// Enforces the version gate for one G6 approval control-plane request.
pub fn validate_policy_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_policy_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: IPC_PROTOCOL_VERSION_V3,
        })
    }
}

pub fn validate_human_decision_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_human_decision_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: IPC_PROTOCOL_VERSION_V4,
        })
    }
}

pub fn validate_planning_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_planning_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: IPC_PROTOCOL_VERSION_V5,
        })
    }
}

pub fn validate_task_authoring_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_task_authoring_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: IPC_PROTOCOL_VERSION_V6,
        })
    }
}

#[must_use]
pub fn is_finder_allowed(negotiated_version: u32) -> bool {
    ipc_contracts::supports_finder_ipc(negotiated_version)
}

pub fn validate_finder_allowed(negotiated_version: u32) -> ControlResult<()> {
    if is_finder_allowed(negotiated_version) {
        Ok(())
    } else {
        Err(ControlError::UnsupportedProtocol {
            negotiated: negotiated_version,
            required: ipc_contracts::IPC_PROTOCOL_VERSION_V7,
        })
    }
}

/// Negotiated handshake parameters on success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedParameters {
    /// Version both sides advertised; the one to use.
    pub selected_version: u32,
    /// Negotiated subset of the requested capabilities (G1: always empty).
    pub accepted_capabilities: Vec<String>,
}

/// Evaluates a handshake request against server configuration.
///
/// Negotiation outcomes (unknown kind, no common version, empty fields)
/// return [`ControlError`] faults that the session layer renders as
/// `HandshakeRejected` wire responses. Build-version mismatch alone never
/// fails: protocol version governs compatibility, never build metadata.
pub fn evaluate_handshake(
    request: &HandshakeRequest,
    config: &ServerConfig,
) -> ControlResult<AcceptedParameters> {
    if request.supported_protocol_versions.is_empty() {
        return Err(ControlError::InvalidHandshake(
            "supported_protocol_versions must be non-empty".to_owned(),
        ));
    }
    if !is_recognized_client_kind(&request.client_kind) {
        return Err(ControlError::UnknownClientKind(request.client_kind.clone()));
    }
    if request.client_build_version.is_empty() {
        return Err(ControlError::InvalidHandshake(
            "client_build_version must be non-empty".to_owned(),
        ));
    }
    let selected_version = select_highest_common_version(
        &request.supported_protocol_versions,
        &config.supported_versions,
    )
    .ok_or_else(|| ControlError::IncompatibleProtocol {
        client: request.supported_protocol_versions.clone(),
        daemon: config.supported_versions.clone(),
    })?;
    Ok(AcceptedParameters {
        selected_version,
        accepted_capabilities: negotiate_capabilities(
            &request.requested_capabilities,
            &config.supported_capabilities,
        ),
    })
}

/// Renders a negotiation fault as a `HandshakeRejected` wire message with a
/// diagnostic reason plus the daemon version set.
#[must_use]
pub fn rejection_for(fault: &ControlError, daemon_versions: &[u32]) -> HandshakeRejected {
    let reason = match fault {
        ControlError::UnknownClientKind(kind) => {
            format!("unknown client_kind '{kind}'; G1 accepts 'tauri-host' and 'agentctl'")
        }
        ControlError::IncompatibleProtocol { client, daemon } => format!(
            "no common protocol version (client {client:?}, daemon {daemon:?}); \
             negotiated version governs compatibility"
        ),
        ControlError::InvalidHandshake(detail) => format!("invalid handshake: {detail}"),
        other => format!("handshake rejected: {other}"),
    };
    HandshakeRejected {
        reason,
        daemon_supported_protocol_versions: daemon_versions.to_vec(),
    }
}

/// Server-side protocol configuration. No lifecycle, no spawn, no policy.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Protocol versions the daemon speaks. G1 default is exactly `[0]`,
    /// coherent with [`IPC_PROTOCOL_VERSION`] by construction.
    pub supported_versions: Vec<u32>,
    /// Daemon build version: diagnostic metadata, never compatibility.
    pub daemon_build_version: String,
    /// Optional capabilities the daemon supports. G1: empty set.
    pub supported_capabilities: Vec<String>,
}

impl ServerConfig {
    /// Accepted G1 server configuration: protocol 0 only. Retained so
    /// old-daemon behavior stays simulatable in tests.
    pub fn g1_default() -> Self {
        Self {
            supported_versions: vec![IPC_PROTOCOL_VERSION_V0],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }

    /// G2 daemon configuration (ADR-0021): protocols 0 and 1. Old G1
    /// clients offering `[0]` negotiate 0 and keep working; new clients
    /// offering `[0, 1]` negotiate 1. Production default from TASK-0028
    /// on; this task only defines the profile.
    pub fn g2_default() -> Self {
        Self {
            supported_versions: vec![IPC_PROTOCOL_VERSION_V0, IPC_PROTOCOL_VERSION_V1],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }

    /// G5 daemon configuration (TASK-0047): protocols 0, 1, and 2. A v2
    /// session carries both business (`task_core`) and execution
    /// (`execution_core`) authority; older sessions keep their exact
    /// prior authority. Production default from TASK-0047 on.
    pub fn g5_default() -> Self {
        Self {
            supported_versions: vec![
                IPC_PROTOCOL_VERSION_V0,
                IPC_PROTOCOL_VERSION_V1,
                ipc_contracts::IPC_PROTOCOL_VERSION_V2,
            ],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }

    /// G6 daemon configuration: protocols 0 through 3. v3 adds the bounded
    /// approval control plane while preserving all older negotiated surfaces.
    pub fn g6_default() -> Self {
        Self {
            supported_versions: vec![
                IPC_PROTOCOL_VERSION_V0,
                IPC_PROTOCOL_VERSION_V1,
                IPC_PROTOCOL_VERSION_V2,
                IPC_PROTOCOL_VERSION_V3,
            ],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }

    /// G7 daemon configuration: v4 adds bounded Human Decision Queue
    /// observation/resolution, v5 adds planning operations and v6 adds Task
    /// planning-metadata authoring while preserving all historical surfaces.
    pub fn g7_default() -> Self {
        Self {
            supported_versions: vec![
                IPC_PROTOCOL_VERSION_V0,
                IPC_PROTOCOL_VERSION_V1,
                IPC_PROTOCOL_VERSION_V2,
                IPC_PROTOCOL_VERSION_V3,
                IPC_PROTOCOL_VERSION_V4,
                IPC_PROTOCOL_VERSION_V5,
                IPC_PROTOCOL_VERSION_V6,
            ],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }

    /// G9 daemon configuration: v7 adds bounded repository finder pages
    /// and span reopens while preserving all historical surfaces.
    pub fn g9_default() -> Self {
        Self {
            supported_versions: vec![
                IPC_PROTOCOL_VERSION_V0,
                IPC_PROTOCOL_VERSION_V1,
                IPC_PROTOCOL_VERSION_V2,
                IPC_PROTOCOL_VERSION_V3,
                IPC_PROTOCOL_VERSION_V4,
                IPC_PROTOCOL_VERSION_V5,
                IPC_PROTOCOL_VERSION_V6,
                ipc_contracts::IPC_PROTOCOL_VERSION_V7,
            ],
            daemon_build_version: default_daemon_build_version(),
            supported_capabilities: Vec::new(),
        }
    }
}

/// Default daemon build version: the canonical product development
/// identity (`product/version.toml`), e.g. `0.1.0-dev.task0029`.
/// Diagnostic metadata only — never protocol compatibility. Preferred
/// wiring is `agentd`'s own version via [`ServerConfig`]; this helper
/// covers tests and early integration.
#[must_use]
pub fn default_daemon_build_version() -> String {
    product_version::development_version().to_owned()
}

/// Default client build version: the canonical product development
/// identity, same rule as the daemon side.
#[must_use]
pub fn default_client_build_version() -> String {
    product_version::development_version().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc_contracts::IPC_PROTOCOL_VERSION;

    fn g1_config() -> ServerConfig {
        ServerConfig::g1_default()
    }

    fn request(versions: &[u32], kind: &str, build: &str, caps: &[&str]) -> HandshakeRequest {
        HandshakeRequest {
            supported_protocol_versions: versions.to_vec(),
            client_kind: kind.to_owned(),
            client_build_version: build.to_owned(),
            requested_capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn version_selection_matrix() {
        assert_eq!(select_highest_common_version(&[0], &[0]), Some(0));
        assert_eq!(select_highest_common_version(&[0, 1], &[0]), Some(0));
        assert_eq!(select_highest_common_version(&[0, 1, 2], &[0, 2]), Some(2));
        assert_eq!(select_highest_common_version(&[1], &[0]), None);
        assert_eq!(select_highest_common_version(&[], &[0]), None);
        assert_eq!(select_highest_common_version(&[0], &[]), None);
    }

    #[test]
    fn version_selection_ignores_ordering_and_duplicates() {
        assert_eq!(
            select_highest_common_version(&[2, 0, 2, 1], &[1, 0, 1]),
            Some(1)
        );
        assert_eq!(
            select_high_common_sorted(&[1, 0, 2, 2], &[2, 0]),
            select_highest_common_version(&[0, 1, 2], &[0, 2])
        );
    }

    fn select_high_common_sorted(client: &[u32], daemon: &[u32]) -> Option<u32> {
        let mut client_sorted = client.to_vec();
        let mut daemon_sorted = daemon.to_vec();
        client_sorted.sort_unstable();
        daemon_sorted.sort_unstable();
        select_highest_common_version(&client_sorted, &daemon_sorted)
    }

    #[test]
    fn g1_daemon_set_is_zero_and_coherent() {
        assert_eq!(IPC_PROTOCOL_VERSION_V0, 0);
        assert_eq!(g1_config().supported_versions, vec![0]);
        assert_eq!(
            g1_config().supported_versions,
            vec![IPC_PROTOCOL_VERSION_V0]
        );
    }

    #[test]
    fn g2_daemon_serves_both_versions_with_v1_current() {
        let g2 = ServerConfig::g2_default();
        assert_eq!(g2.supported_versions, vec![0, 1]);
        // The global current protocol may advance; the frozen G2 profile
        // itself still offers exactly [0, 1].
        assert_eq!(g2.supported_versions.last().copied(), Some(1));
        // Old G1 client [0] x new daemon [0,1] -> 0.
        let old_client = request(&[0], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&old_client, &g2).expect("old client stays valid");
        assert_eq!(params.selected_version, 0);
        assert!(
            !is_business_allowed(params.selected_version),
            "v0 session must stay bootstrap-only"
        );
        // New client [0,1] x new daemon [0,1] -> 1.
        let new_client = request(&[0, 1], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&new_client, &g2).expect("new client negotiates");
        assert_eq!(params.selected_version, 1);
        assert!(is_business_allowed(params.selected_version));
    }

    #[test]
    fn business_gate_rejects_v0_without_executing() {
        assert!(validate_business_allowed(0).is_err());
        assert!(validate_business_allowed(1).is_ok());
        match validate_business_allowed(0) {
            Err(ControlError::UnsupportedProtocol {
                negotiated,
                required,
            }) => {
                assert_eq!(negotiated, 0);
                assert_eq!(required, 1);
            }
            other => panic!("expected UnsupportedProtocol, got {other:?}"),
        }
        assert!(!is_business_allowed(0));
        assert!(is_business_allowed(1));
        assert!(is_business_allowed(2));
        assert!(!is_business_allowed(99));
    }

    #[test]
    fn execution_gate_allows_v2_and_later_cumulative_v3() {
        assert!(validate_execution_allowed(2).is_ok());
        assert!(validate_execution_allowed(3).is_ok());
        assert!(validate_execution_allowed(0).is_err());
        assert!(validate_execution_allowed(1).is_err());
        match validate_execution_allowed(1) {
            Err(ControlError::UnsupportedProtocol {
                negotiated,
                required,
            }) => {
                assert_eq!(negotiated, 1);
                assert_eq!(required, ipc_contracts::IPC_PROTOCOL_VERSION_V2);
            }
            other => panic!("expected UnsupportedProtocol, got {other:?}"),
        }
        assert!(!is_execution_allowed(0));
        assert!(!is_execution_allowed(1));
        assert!(is_execution_allowed(2));
        assert!(is_execution_allowed(3));
        assert!(!is_execution_allowed(99));
    }

    #[test]
    fn g5_daemon_serves_all_versions_with_v2_current() {
        let g5 = ServerConfig::g5_default();
        assert_eq!(g5.supported_versions, vec![0, 1, 2]);
        assert_eq!(g5.supported_versions.last().copied(), Some(2));
        // Old G1 client [0] x new daemon [0,1,2] -> 0 (bootstrap only).
        let old_client = request(&[0], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&old_client, &g5).expect("old client stays valid");
        assert_eq!(params.selected_version, 0);
        assert!(!is_business_allowed(params.selected_version));
        assert!(!is_execution_allowed(params.selected_version));
        // G2 client [0,1] x new daemon -> 1 (business, no execution).
        let g2_client = request(&[0, 1], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&g2_client, &g5).expect("g2 client negotiates");
        assert_eq!(params.selected_version, 1);
        assert!(is_business_allowed(params.selected_version));
        assert!(!is_execution_allowed(params.selected_version));
        // New client [0,1,2] x new daemon -> 2 (both).
        let new_client = request(&[0, 1, 2], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&new_client, &g5).expect("new client negotiates");
        assert_eq!(params.selected_version, 2);
        assert!(is_business_allowed(params.selected_version));
        assert!(is_execution_allowed(params.selected_version));
    }

    #[test]
    fn g6_daemon_adds_v3_policy_without_weakening_older_profiles() {
        let g6 = ServerConfig::g6_default();
        assert_eq!(g6.supported_versions, vec![0, 1, 2, 3]);
        assert_eq!(IPC_PROTOCOL_VERSION, ipc_contracts::IPC_PROTOCOL_VERSION_V7);
        assert!(validate_policy_allowed(3).is_ok());
        assert!(validate_policy_allowed(2).is_err());
        assert!(!is_policy_allowed(0));
        assert!(!is_policy_allowed(1));
        assert!(!is_policy_allowed(2));
        assert!(is_policy_allowed(3));

        let policy_client = request(&[0, 1, 2, 3], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&policy_client, &g6).expect("v3 client negotiates");
        assert_eq!(params.selected_version, 3);
        assert!(is_business_allowed(3));
        assert!(is_execution_allowed(3));
        assert!(is_policy_allowed(3));
    }

    #[test]
    fn g7_daemon_adds_v4_human_decisions_without_weakening_older_profiles() {
        let g7 = ServerConfig::g7_default();
        assert_eq!(g7.supported_versions, vec![0, 1, 2, 3, 4, 5, 6]);
        assert!(validate_human_decision_allowed(4).is_ok());
        assert!(validate_human_decision_allowed(3).is_err());
        assert!(is_business_allowed(4));
        assert!(is_execution_allowed(4));
        assert!(is_policy_allowed(4));
        assert!(is_human_decision_allowed(4));

        let client = request(&[0, 1, 2, 3, 4], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&client, &g7).expect("v4 client negotiates");
        assert_eq!(params.selected_version, 4);
    }

    #[test]
    fn v5_planning_operations_are_unavailable_to_historical_clients() {
        let daemon = ServerConfig::g7_default();
        let historical = request(&[0, 1, 2, 3, 4], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&historical, &daemon).expect("v4 remains compatible");
        assert_eq!(params.selected_version, 4);
        assert!(!is_planning_allowed(params.selected_version));
        assert!(matches!(
            validate_planning_allowed(params.selected_version),
            Err(ControlError::UnsupportedProtocol {
                negotiated: 4,
                required: 5
            })
        ));

        let v5 = request(&[0, 1, 2, 3, 4, 5], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&v5, &daemon).expect("v5 client negotiates");
        assert_eq!(params.selected_version, 5);
        assert!(validate_planning_allowed(params.selected_version).is_ok());
    }

    #[test]
    fn v6_task_authoring_is_unavailable_to_historical_clients() {
        let daemon = ServerConfig::g7_default();
        let v5 = request(&[0, 1, 2, 3, 4, 5], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&v5, &daemon).expect("v5 remains compatible");
        assert_eq!(params.selected_version, 5);
        assert!(is_planning_allowed(params.selected_version));
        assert!(!is_task_authoring_allowed(params.selected_version));
        assert!(matches!(
            validate_task_authoring_allowed(params.selected_version),
            Err(ControlError::UnsupportedProtocol {
                negotiated: 5,
                required: 6
            })
        ));

        let v6 = request(&[0, 1, 2, 3, 4, 5, 6], "agentctl", "0.1.0", &[]);
        let params = evaluate_handshake(&v6, &daemon).expect("v6 client negotiates");
        assert_eq!(params.selected_version, 6);
        assert!(validate_task_authoring_allowed(6).is_ok());
        assert!(validate_planning_allowed(6).is_ok());
        assert!(validate_human_decision_allowed(6).is_ok());
        assert!(validate_policy_allowed(6).is_ok());
        assert!(validate_execution_allowed(6).is_ok());
        assert!(validate_business_allowed(6).is_ok());
    }

    #[test]
    fn build_versions_report_canonical_development_identity() {
        // Both sides report the single canonical product identity from
        // product/version.toml — never diverging package versions and
        // never a hardcoded historical task literal. Rolling the version
        // to the next task must not require editing this test.
        assert_eq!(
            default_daemon_build_version(),
            product_version::development_version()
        );
        assert_eq!(
            default_client_build_version(),
            product_version::development_version()
        );
    }

    #[test]
    fn build_version_mismatch_is_allowed_on_v1_too() {
        let params = evaluate_handshake(
            &request(&[0, 1], "agentctl", "9.9.9-future", &[]),
            &ServerConfig::g2_default(),
        )
        .expect("build mismatch must not reject on v1");
        assert_eq!(params.selected_version, 1);
    }

    #[test]
    fn client_kind_registry_is_exact() {
        assert!(is_recognized_client_kind("tauri-host"));
        assert!(is_recognized_client_kind("agentctl"));
        assert!(!is_recognized_client_kind("evil-test-client"));
        assert!(!is_recognized_client_kind(""));
        assert!(!is_recognized_client_kind("Tauri-Host"));
        assert!(!is_recognized_client_kind(" agentctl"));
    }

    #[test]
    fn build_version_mismatch_is_allowed() {
        let params = evaluate_handshake(
            &request(&[0], "agentctl", "9.9.9-future", &[]),
            &g1_config(),
        )
        .expect("build mismatch must not reject");
        assert_eq!(params.selected_version, 0);
    }

    #[test]
    fn unknown_capabilities_are_omitted_not_rejected() {
        let params = evaluate_handshake(
            &request(&[0], "agentctl", "0.1.0", &["unknown-capability"]),
            &g1_config(),
        )
        .expect("unknown caps must not reject");
        assert!(params.accepted_capabilities.is_empty());
    }

    #[test]
    fn capability_intersection_is_generic() {
        let config = ServerConfig {
            supported_versions: vec![0],
            daemon_build_version: "x".to_owned(),
            supported_capabilities: vec!["b".to_owned(), "a".to_owned()],
        };
        assert_eq!(
            negotiate_capabilities(
                &[
                    "a".to_owned(),
                    "zzz".to_owned(),
                    "b".to_owned(),
                    "a".to_owned()
                ],
                &config.supported_capabilities,
            ),
            vec!["a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn handshake_faults_are_typed() {
        assert!(matches!(
            evaluate_handshake(&request(&[], "agentctl", "0.1.0", &[]), &g1_config()),
            Err(ControlError::InvalidHandshake(_))
        ));
        assert!(matches!(
            evaluate_handshake(&request(&[0], "agentctl", "", &[]), &g1_config()),
            Err(ControlError::InvalidHandshake(_))
        ));
        assert!(matches!(
            evaluate_handshake(&request(&[0], "evil-test-client", "0.1.0", &[]), &g1_config()),
            Err(ControlError::UnknownClientKind(kind)) if kind == "evil-test-client"
        ));
        assert!(matches!(
            evaluate_handshake(&request(&[99], "agentctl", "0.1.0", &[]), &g1_config()),
            Err(ControlError::IncompatibleProtocol { .. })
        ));
    }

    #[test]
    fn rejection_carries_reason_and_daemon_versions() {
        let fault = ControlError::IncompatibleProtocol {
            client: vec![99],
            daemon: vec![0],
        };
        let rejected = rejection_for(&fault, &[0]);
        assert!(!rejected.reason.is_empty());
        assert_eq!(rejected.daemon_supported_protocol_versions, vec![0]);
    }
}
