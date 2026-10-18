//! Memory IPC dispatch (TASK-0184, G10): `MemoryCoreRequest` in,
//! `MemoryCoreResponse` out. Pure wire mapping plus validation over
//! [`MemoryService`]; lifecycle and authority stay in the domain and
//! service layers.

use app_services::{CaptureRequest, MemoryService};
use ipc_contracts::proto::{
    memory_core_request, memory_core_response, MemoryAnchor, MemoryCoreRequest, MemoryCoreResponse,
    MemoryErrorResponse, MemoryListResponse, MemoryRecordResponse, MemorySourceRef,
};
use memory::model::{
    AnchorFingerprint, AnchorKind, MemoryError, MemoryId, MemoryKind, MemoryRecord,
    MemorySensitivity as Sensitivity, SourceRef, SourceRefKind,
};
use memory::port::CaptureOutcome;

fn error_response(code: &str, detail: &str) -> MemoryCoreResponse {
    MemoryCoreResponse {
        payload: Some(memory_core_response::Payload::Error(MemoryErrorResponse {
            code: code.to_owned(),
            detail: detail.chars().take(2048).collect(),
        })),
    }
}

/// Typed unavailable shape for daemons without a memory composition.
pub fn unavailable() -> MemoryCoreResponse {
    error_response("unavailable", "memory composition is not available")
}

fn map_error(error: &MemoryError) -> MemoryCoreResponse {
    let code = match error {
        MemoryError::InvalidInput(_) => "invalid_input",
        MemoryError::UnknownKind(_) => "unknown_kind",
        MemoryError::UnboundSourceRef(_) => "unbound_source_ref",
        MemoryError::SensitivityRefusal { .. } => "sensitivity_refusal",
        MemoryError::UntrustedPromotion(_) => "untrusted_promotion",
        MemoryError::StaleAnchor(_) => "stale_anchor",
        MemoryError::AlreadySuperseded(_) => "already_superseded",
        MemoryError::Conflict(_) => "conflict",
        MemoryError::NotFound(_) => "not_found",
        MemoryError::BackendFailure { .. } => "backend_failure",
    };
    error_response(code, &error.to_string())
}

fn record_response(record: &MemoryRecord, deduplicated: bool) -> MemoryRecordResponse {
    MemoryRecordResponse {
        memory_id: record.id.to_string(),
        kind: record.kind.as_str().to_owned(),
        content: record.content.clone(),
        content_hash: record.content_hash.clone(),
        confidence: u32::from(record.confidence),
        sensitivity: record.sensitivity.as_str().to_owned(),
        trust_state: record.trust_state.as_str().to_owned(),
        author_ref: record.author_ref.clone(),
        created_at: record.created_at,
        last_verified_at: record.last_verified_at,
        expires_at: record.expires_at.unwrap_or(0),
        supersedes: record
            .supersedes
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        deduplicated,
    }
}

fn parse_id(raw: &str) -> Result<MemoryId, MemoryError> {
    MemoryId::new(raw.to_owned())
}

fn parse_sources(refs: &[MemorySourceRef]) -> Result<Vec<SourceRef>, MemoryError> {
    refs.iter()
        .map(|r| {
            let kind = SourceRefKind::parse(&r.kind)?;
            SourceRef::new(kind, r.reference.clone(), r.revision.clone())
        })
        .collect()
}

fn parse_anchors(anchors: &[MemoryAnchor]) -> Result<Vec<AnchorFingerprint>, MemoryError> {
    anchors
        .iter()
        .map(|a| {
            let kind = AnchorKind::parse(&a.kind)?;
            AnchorFingerprint::new(kind, a.reference.clone(), a.fingerprint.clone())
        })
        .collect()
}

/// Dispatches one memory request against the composed service.
pub fn dispatch<S: memory::port::MemoryStorePort>(
    service: &mut MemoryService<S>,
    request: MemoryCoreRequest,
) -> MemoryCoreResponse {
    match request.payload {
        Some(memory_core_request::Payload::Capture(capture)) => {
            let kind = match MemoryKind::parse(&capture.kind) {
                Ok(kind) => kind,
                Err(e) => return map_error(&e),
            };
            let sensitivity = match Sensitivity::parse(&capture.sensitivity) {
                Ok(s) => s,
                Err(_) => {
                    return error_response(
                        "invalid_input",
                        &format!("unknown sensitivity: {}", capture.sensitivity),
                    )
                }
            };
            let source_refs = match parse_sources(&capture.source_refs) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            let anchors = match parse_anchors(&capture.anchors) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            let confidence = match u16::try_from(capture.confidence) {
                Ok(v) if v <= 1000 => v,
                _ => return error_response("invalid_input", "confidence must be 0..=1000"),
            };
            let supersedes = if capture.supersedes.is_empty() {
                None
            } else {
                match parse_id(&capture.supersedes) {
                    Ok(id) => Some(id),
                    Err(e) => return map_error(&e),
                }
            };
            let built = CaptureRequest {
                kind,
                content: capture.content,
                source_refs,
                confidence,
                sensitivity,
                anchors,
                author_ref: capture.author_ref,
                expires_at: (capture.expires_at != 0).then_some(capture.expires_at),
                supersedes,
            };
            match service.capture(built, capture.now_millis) {
                Ok(CaptureOutcome {
                    record,
                    deduplicated,
                }) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::Record(record_response(
                        &record,
                        deduplicated,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(memory_core_request::Payload::Get(get)) => {
            let id = match parse_id(&get.memory_id) {
                Ok(id) => id,
                Err(e) => return map_error(&e),
            };
            match service.get(&id) {
                Ok(record) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::Record(record_response(
                        &record, false,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(memory_core_request::Payload::Validate(validate)) => {
            let id = match parse_id(&validate.memory_id) {
                Ok(id) => id,
                Err(e) => return map_error(&e),
            };
            match service.validate(&id, &validate.validator_ref, validate.now_millis) {
                Ok(record) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::Record(record_response(
                        &record, false,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(memory_core_request::Payload::Supersede(supersede)) => {
            let old = match parse_id(&supersede.old_memory_id) {
                Ok(id) => id,
                Err(e) => return map_error(&e),
            };
            let successor = match parse_id(&supersede.successor_id) {
                Ok(id) => id,
                Err(e) => return map_error(&e),
            };
            match service.supersede(&old, &successor) {
                Ok(record) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::Record(record_response(
                        &record, false,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(memory_core_request::Payload::Retire(retire)) => {
            let id = match parse_id(&retire.memory_id) {
                Ok(id) => id,
                Err(e) => return map_error(&e),
            };
            match service.retire(&id) {
                Ok(record) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::Record(record_response(
                        &record, false,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(memory_core_request::Payload::ListEligible(list)) => {
            match service.list_eligible(list.now_millis) {
                Ok(records) => MemoryCoreResponse {
                    payload: Some(memory_core_response::Payload::List(MemoryListResponse {
                        records: records.iter().map(|r| record_response(r, false)).collect(),
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        None => error_response("invalid_input", "missing memory payload"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc_contracts::proto::{
        MemoryCaptureRequest, MemoryGetRequest, MemoryListEligibleRequest, MemoryValidateRequest,
    };
    use memory::port::MemoryStorePort;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemStore {
        inner: Arc<Mutex<BTreeMap<MemoryId, MemoryRecord>>>,
    }

    impl MemoryStorePort for MemStore {
        fn capture(&mut self, record: &MemoryRecord) -> Result<CaptureOutcome, MemoryError> {
            record.revalidate()?;
            let mut inner = self.inner.lock().unwrap();
            if let Some(existing) = inner.values().find(|r| r.dedup_key() == record.dedup_key()) {
                return Ok(CaptureOutcome {
                    record: existing.clone(),
                    deduplicated: true,
                });
            }
            inner.insert(record.id.clone(), record.clone());
            Ok(CaptureOutcome {
                record: record.clone(),
                deduplicated: false,
            })
        }
        fn load(&self, id: &MemoryId) -> Result<Option<MemoryRecord>, MemoryError> {
            Ok(self.inner.lock().unwrap().get(id).cloned())
        }
        fn load_by_dedup_key(&self, key: &str) -> Result<Option<MemoryRecord>, MemoryError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .values()
                .find(|r| r.dedup_key() == key)
                .cloned())
        }
        fn update(&mut self, record: &MemoryRecord) -> Result<(), MemoryError> {
            let mut inner = self.inner.lock().unwrap();
            if !inner.contains_key(&record.id) {
                return Err(MemoryError::NotFound(record.id.to_string()));
            }
            inner.insert(record.id.clone(), record.clone());
            Ok(())
        }
        fn link(&mut self, _link: &memory::model::MemoryLink) -> Result<(), MemoryError> {
            Ok(())
        }
        fn links_from(
            &self,
            _from: &MemoryId,
        ) -> Result<Vec<memory::model::MemoryLink>, MemoryError> {
            Ok(Vec::new())
        }
        fn upsert_entity(&mut self, _e: &memory::model::Entity) -> Result<(), MemoryError> {
            Ok(())
        }
        fn load_entity(
            &self,
            _id: &memory::model::EntityId,
        ) -> Result<Option<memory::model::Entity>, MemoryError> {
            Ok(None)
        }
        fn link_entity(&mut self, _l: &memory::model::EntityLink) -> Result<(), MemoryError> {
            Ok(())
        }
        fn entities_for(&self, _m: &MemoryId) -> Result<Vec<memory::model::Entity>, MemoryError> {
            Ok(Vec::new())
        }
        fn list_eligible(&self, now: u64) -> Result<Vec<MemoryRecord>, MemoryError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .values()
                .filter(|r| r.is_context_eligible(now))
                .cloned()
                .collect())
        }
    }

    fn fp(byte: &str) -> String {
        format!("sha256:{}", byte.repeat(32))
    }

    #[test]
    fn capture_validate_list_round_trip_and_author_refusal() {
        let mut service = MemoryService::new(MemStore::default());
        let capture = MemoryCoreRequest {
            payload: Some(memory_core_request::Payload::Capture(
                MemoryCaptureRequest {
                    kind: "SEMANTIC".to_owned(),
                    content: "fact".to_owned(),
                    source_refs: vec![MemorySourceRef {
                        kind: "TASK".to_owned(),
                        reference: "TASK-1".to_owned(),
                        revision: "rev1".to_owned(),
                    }],
                    confidence: 900,
                    sensitivity: "PROJECT".to_owned(),
                    anchors: vec![MemoryAnchor {
                        kind: "CODE".to_owned(),
                        reference: "crates/x".to_owned(),
                        fingerprint: fp("ab"),
                    }],
                    author_ref: "agent-a".to_owned(),
                    now_millis: 1_000,
                    expires_at: 0,
                    supersedes: String::new(),
                },
            )),
        };
        let response = dispatch(&mut service, capture.clone());
        let id = match response.payload {
            Some(memory_core_response::Payload::Record(r)) => {
                assert_eq!(r.trust_state, "PROPOSED");
                r.memory_id
            }
            other => panic!("expected record, got {other:?}"),
        };
        // Author self-validation refuses typed.
        let refused = dispatch(
            &mut service,
            MemoryCoreRequest {
                payload: Some(memory_core_request::Payload::Validate(
                    MemoryValidateRequest {
                        memory_id: id.clone(),
                        validator_ref: "agent-a".to_owned(),
                        now_millis: 2_000,
                    },
                )),
            },
        );
        assert!(matches!(
            refused.payload,
            Some(memory_core_response::Payload::Error(e)) if e.code == "untrusted_promotion"
        ));
        dispatch(
            &mut service,
            MemoryCoreRequest {
                payload: Some(memory_core_request::Payload::Validate(
                    MemoryValidateRequest {
                        memory_id: id.clone(),
                        validator_ref: "reviewer".to_owned(),
                        now_millis: 2_000,
                    },
                )),
            },
        );
        let listed = dispatch(
            &mut service,
            MemoryCoreRequest {
                payload: Some(memory_core_request::Payload::ListEligible(
                    MemoryListEligibleRequest { now_millis: 3_000 },
                )),
            },
        );
        assert!(matches!(
            listed.payload,
            Some(memory_core_response::Payload::List(l)) if l.records.len() == 1
        ));
        let got = dispatch(
            &mut service,
            MemoryCoreRequest {
                payload: Some(memory_core_request::Payload::Get(MemoryGetRequest {
                    memory_id: id,
                })),
            },
        );
        assert!(matches!(
            got.payload,
            Some(memory_core_response::Payload::Record(r)) if r.trust_state == "CONFIRMED"
        ));
        // SECRET capture refuses typed.
        let mut secret = capture;
        if let Some(memory_core_request::Payload::Capture(c)) = &mut secret.payload {
            c.sensitivity = "SECRET".to_owned();
            c.content = "hidden".to_owned();
        }
        let refused = dispatch(&mut service, secret);
        assert!(matches!(
            refused.payload,
            Some(memory_core_response::Payload::Error(e)) if e.code == "sensitivity_refusal"
        ));
    }
}
