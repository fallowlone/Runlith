//! Org IPC dispatch (TASK-0211, G13): OrgCoreRequest in,
//! OrgCoreResponse out over OrgService (ADR-0047).
use app_services::org_service::{OrgService, PolicyRequest, UsageRequest};
use ipc_contracts::proto::{
    org_core_request, org_core_response, OrgAccountResponse, OrgCoreRequest, OrgCoreResponse,
    OrgErrorResponse, OrgHeadResponse, OrgListResponse, OrgMembershipResponse,
    OrgOrganizationResponse, OrgPlaintextResponse, OrgPolicyResponse, OrgTeamResponse,
    OrgUsageResponse,
};
use org::model::{
    AccountId, AccountRecord, CapabilityCeiling, CipherSuite, EnvelopeId, EnvelopeKind,
    MembershipId, MembershipRecord, OrgError, OrgPolicy, OrganizationId, OrganizationRecord, Role,
    Sensitivity, SyncEnvelope, TeamId, TeamRecord, UsageEntry, UsageKind,
};
use org::port::OrgStorePort;

fn error_response(code: &str, detail: &str) -> OrgCoreResponse {
    OrgCoreResponse {
        payload: Some(org_core_response::Payload::Error(OrgErrorResponse {
            code: code.to_owned(),
            detail: detail.chars().take(2048).collect(),
        })),
    }
}

pub fn unavailable() -> OrgCoreResponse {
    error_response("unavailable", "org composition is not available")
}

fn map_error(error: &OrgError) -> OrgCoreResponse {
    let code = match error {
        OrgError::InvalidInput(_) => "invalid_input",
        OrgError::UnknownKind(_) => "unknown_kind",
        OrgError::ForbiddenRole(_) => "forbidden_role",
        OrgError::SelfRoleChange(_) => "self_role_change",
        OrgError::RevokedMember(_) => "revoked_member",
        OrgError::UnknownSigner(_) => "unknown_signer",
        OrgError::BadSignature(_) => "bad_signature",
        OrgError::TamperedEnvelope(_) => "tampered_envelope",
        OrgError::Conflict(_) => "conflict",
        OrgError::DuplicateEnvelope(_) => "duplicate_envelope",
        OrgError::SensitivityRefusal(_) => "sensitivity_refusal",
        OrgError::PolicyWeakening(_) => "policy_weakening",
        OrgError::OverQuota(_) => "over_quota",
        OrgError::NotFound(_) => "not_found",
        OrgError::BackendFailure { .. } => "backend_failure",
    };
    error_response(code, &error.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], OrgError> {
    if raw.len() != N * 2 {
        return Err(OrgError::InvalidInput(format!(
            "{what} must be {} hex chars",
            N * 2
        )));
    }
    let bytes: Vec<u8> = (0..N)
        .map(|i| {
            u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
                .map_err(|_| OrgError::InvalidInput(format!("{what} is not hex")))
        })
        .collect::<Result<Vec<u8>, OrgError>>()?;
    bytes
        .try_into()
        .map_err(|_| OrgError::InvalidInput(format!("{what} has wrong length")))
}

fn hex_decode_vec(raw: &str, what: &str) -> Result<Vec<u8>, OrgError> {
    if !raw.len().is_multiple_of(2) {
        return Err(OrgError::InvalidInput(format!("{what} is not hex")));
    }
    (0..raw.len() / 2)
        .map(|i| {
            u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
                .map_err(|_| OrgError::InvalidInput(format!("{what} is not hex")))
        })
        .collect()
}

fn account_response(a: &AccountRecord) -> OrgAccountResponse {
    OrgAccountResponse {
        account_id: a.id.to_string(),
        display_name: a.display_name.clone(),
        public_key_hex: hex_encode(&a.signing_public_key),
        fingerprint: a.fingerprint.clone(),
        created_at: a.created_at,
    }
}

fn org_response(o: &OrganizationRecord, owner_membership_id: &str) -> OrgOrganizationResponse {
    OrgOrganizationResponse {
        org_id: o.id.to_string(),
        name: o.name.clone(),
        state: o.state.as_str().to_owned(),
        owner_membership_id: owner_membership_id.to_owned(),
    }
}

fn team_response(t: &TeamRecord) -> OrgTeamResponse {
    OrgTeamResponse {
        team_id: t.id.to_string(),
        org_id: t.org_id.to_string(),
        name: t.name.clone(),
    }
}

fn membership_response(m: &MembershipRecord) -> OrgMembershipResponse {
    OrgMembershipResponse {
        membership_id: m.id.to_string(),
        org_id: m.org_id.to_string(),
        account_id: m.account_id.to_string(),
        team_id: m
            .team_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        role: m.role.as_str().to_owned(),
        state: m.state.as_str().to_owned(),
    }
}

fn policy_response(p: &OrgPolicy) -> OrgPolicyResponse {
    OrgPolicyResponse {
        policy_id: p.id.to_string(),
        org_id: p.org_id.to_string(),
        version: p.version,
        hard_capability_ceiling: p.hard_capability_ceiling.as_str().to_owned(),
        sync_max_sensitivity: p.sync_max_sensitivity.as_str().to_owned(),
        quota_max_cost_microusd: p.quota_max_cost_microusd,
        quota_window_seconds: p.quota_window_seconds,
        updated_by: p.updated_by.to_string(),
    }
}

fn usage_response(u: &UsageEntry) -> OrgUsageResponse {
    OrgUsageResponse {
        usage_id: u.id.to_string(),
        org_id: u.org_id.to_string(),
        account_id: u.account_id.to_string(),
        kind: u.kind.as_str().to_owned(),
        units: u.units,
        cost_microusd: u.cost_microusd,
        idempotency_key: u.idempotency_key.clone(),
    }
}

macro_rules! id_or_err {
    ($raw:expr, $ty:ty) => {
        match <$ty>::new($raw) {
            Ok(v) => v,
            Err(e) => return map_error(&e),
        }
    };
}

pub fn dispatch<S: OrgStorePort>(
    service: &mut OrgService<S>,
    request: OrgCoreRequest,
) -> OrgCoreResponse {
    match request.payload {
        Some(org_core_request::Payload::RegisterAccount(req)) => {
            let public = match hex_decode::<32>(&req.public_key_hex, "public_key") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.register_account(&req.display_name, &public, req.now_millis) {
                Ok(a) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Account(account_response(&a))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::GetAccount(req)) => {
            let id = id_or_err!(req.account_id, AccountId);
            match service.get_account(&id) {
                Ok(a) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Account(account_response(&a))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::CreateOrg(req)) => {
            let creator = id_or_err!(req.creator_account_id, AccountId);
            match service.create_organization(&creator, &req.name, req.now_millis) {
                Ok((org, owner)) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Organization(org_response(
                        &org,
                        &owner.id.to_string(),
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::GetOrg(req)) => {
            let id = id_or_err!(req.org_id, OrganizationId);
            match service.organization(&id) {
                Ok(o) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Organization(org_response(
                        &o, "",
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::CreateTeam(req)) => {
            let actor = id_or_err!(req.actor_membership_id, MembershipId);
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.create_team(&actor, &org_id, &req.name, req.now_millis) {
                Ok(t) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Team(team_response(&t))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::ListTeams(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.list_teams(&org_id) {
                Ok(teams) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::List(OrgListResponse {
                        teams: teams.iter().map(team_response).collect(),
                        memberships: Vec::new(),
                        usage: Vec::new(),
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::AddMember(req)) => {
            let actor = id_or_err!(req.actor_membership_id, MembershipId);
            let org_id = id_or_err!(req.org_id, OrganizationId);
            let account = id_or_err!(req.account_id, AccountId);
            let team = if req.team_id.is_empty() {
                None
            } else {
                Some(id_or_err!(req.team_id, TeamId))
            };
            let role = match Role::parse(&req.role) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.add_member(&actor, &org_id, &account, team, role, req.now_millis) {
                Ok(m) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Membership(membership_response(
                        &m,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::ChangeRole(req)) => {
            let actor = id_or_err!(req.actor_membership_id, MembershipId);
            let target = id_or_err!(req.membership_id, MembershipId);
            let role = match Role::parse(&req.role) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.change_role(&actor, &target, role) {
                Ok(m) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Membership(membership_response(
                        &m,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::RevokeMember(req)) => {
            let actor = id_or_err!(req.actor_membership_id, MembershipId);
            let target = id_or_err!(req.membership_id, MembershipId);
            match service.revoke_member(&actor, &target) {
                Ok(m) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Membership(membership_response(
                        &m,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::ListMemberships(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.list_memberships(&org_id) {
                Ok(memberships) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::List(OrgListResponse {
                        teams: Vec::new(),
                        memberships: memberships.iter().map(membership_response).collect(),
                        usage: Vec::new(),
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::WritePolicy(req)) => {
            let actor = id_or_err!(req.actor_membership_id, MembershipId);
            let org_id = id_or_err!(req.org_id, OrganizationId);
            let ceiling = match CapabilityCeiling::parse(&req.hard_capability_ceiling) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            let sync_max = match Sensitivity::parse(&req.sync_max_sensitivity) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.write_policy(
                &actor,
                &org_id,
                PolicyRequest {
                    hard_capability_ceiling: ceiling,
                    sync_max_sensitivity: sync_max,
                    quota_max_cost_microusd: req.quota_max_cost_microusd,
                    quota_window_seconds: req.quota_window_seconds,
                },
                req.now_millis,
            ) {
                Ok(p) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Policy(policy_response(&p))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::GetPolicy(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.current_policy(&org_id) {
                Ok(p) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Policy(policy_response(&p))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::RecordUsage(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            let account = id_or_err!(req.account_id, AccountId);
            let kind = match UsageKind::parse(&req.kind) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.record_usage(
                &org_id,
                UsageRequest {
                    account_id: account,
                    project_ref: if req.project_ref.is_empty() {
                        None
                    } else {
                        Some(req.project_ref)
                    },
                    kind,
                    units: req.units,
                    cost_microusd: req.cost_microusd,
                    idempotency_key: req.idempotency_key,
                },
                req.now_millis,
            ) {
                Ok(u) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Usage(usage_response(&u))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::ListUsage(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.list_usage(&org_id) {
                Ok(usage) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::List(OrgListResponse {
                        teams: Vec::new(),
                        memberships: Vec::new(),
                        usage: usage.iter().map(usage_response).collect(),
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::CheckQuota(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.check_quota(&org_id, req.requested_estimate, req.now_millis) {
                Ok(()) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Quota(
                        ipc_contracts::proto::OrgQuotaResponse {
                            org_id: org_id.to_string(),
                            requested_estimate: req.requested_estimate,
                        },
                    )),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::SyncHead(req)) => {
            let org_id = id_or_err!(req.org_id, OrganizationId);
            match service.sync_head(&org_id) {
                Ok(head) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Head(OrgHeadResponse {
                        org_id: org_id.to_string(),
                        head,
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(org_core_request::Payload::AcceptEnvelope(req)) => {
            let Some(msg) = req.envelope else {
                return error_response("invalid_input", "missing envelope");
            };
            let envelope = match build_envelope(&msg) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            let org_key = match hex_decode::<32>(&req.org_key_hex, "org_key") {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.accept_envelope(&envelope, &org_key) {
                Ok(plaintext) => OrgCoreResponse {
                    payload: Some(org_core_response::Payload::Plaintext(
                        OrgPlaintextResponse {
                            plaintext_hex: hex_encode(&plaintext),
                            revision: envelope.revision,
                        },
                    )),
                },
                Err(e) => map_error(&e),
            }
        }
        None => error_response("invalid_input", "missing org payload"),
    }
}

fn build_envelope(
    msg: &ipc_contracts::proto::OrgEnvelopeMessage,
) -> Result<SyncEnvelope, OrgError> {
    Ok(SyncEnvelope {
        id: EnvelopeId::new(msg.envelope_id.clone())?,
        org_id: OrganizationId::new(msg.org_id.clone())?,
        author_account_id: AccountId::new(msg.author_account_id.clone())?,
        kind: EnvelopeKind::parse(&msg.kind)?,
        revision: msg.revision,
        base_revision: msg.base_revision,
        max_sensitivity: Sensitivity::parse(&msg.max_sensitivity)?,
        cipher_suite: CipherSuite::XChaCha20Poly1305Ed25519,
        nonce: hex_decode::<24>(&msg.nonce_hex, "nonce")?,
        ciphertext: hex_decode_vec(&msg.ciphertext_hex, "ciphertext")?,
        signature: hex_decode::<64>(&msg.signature_hex, "signature")?,
        created_at: msg.created_at,
    })
}
