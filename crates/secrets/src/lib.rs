//! Provider-neutral Secrets Broker contracts for G6 TASK-0063.
//!
//! Secret values stay in a [`SecretBackend`]. Durable application state owns
//! only opaque [`SecretRef`] values and exact [`SecretBinding`] records.
//! Release requires already-decided effective authority and delivers plaintext
//! only to the caller-supplied trusted execution/tool boundary.

use std::fmt;

use policy::{Capability, EffectiveCapabilities, PolicyContractError, ScopeId};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretRef(ScopeId);

impl SecretRef {
    pub fn new(value: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self(ScopeId::new(value)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn capability(&self) -> Capability {
        Capability::SecretAccess(self.0.clone())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretBindingId(ScopeId);

impl SecretBindingId {
    pub fn new(value: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self(ScopeId::new(value)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecretDeliveryKind {
    Execution,
    Tool,
}

impl SecretDeliveryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Execution => "execution",
            Self::Tool => "tool",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SecretMetadataError> {
        match value {
            "execution" => Ok(Self::Execution),
            "tool" => Ok(Self::Tool),
            _ => Err(SecretMetadataError::Corrupt),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretDeliveryTarget {
    kind: SecretDeliveryKind,
    id: ScopeId,
}

impl SecretDeliveryTarget {
    pub fn execution(id: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self {
            kind: SecretDeliveryKind::Execution,
            id: ScopeId::new(id)?,
        })
    }

    pub fn tool(id: impl Into<String>) -> Result<Self, PolicyContractError> {
        Ok(Self {
            kind: SecretDeliveryKind::Tool,
            id: ScopeId::new(id)?,
        })
    }

    pub const fn kind(&self) -> SecretDeliveryKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretBinding {
    id: SecretBindingId,
    secret: SecretRef,
    target: SecretDeliveryTarget,
}

impl SecretBinding {
    pub fn new(id: SecretBindingId, secret: SecretRef, target: SecretDeliveryTarget) -> Self {
        Self { id, secret, target }
    }

    pub fn id(&self) -> &SecretBindingId {
        &self.id
    }

    pub fn secret(&self) -> &SecretRef {
        &self.secret
    }

    pub fn target(&self) -> &SecretDeliveryTarget {
        &self.target
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretMetadataError {
    NotFound,
    Conflict,
    Corrupt,
    Unavailable,
}

impl fmt::Display for SecretMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "secret metadata not found",
            Self::Conflict => "secret metadata conflict",
            Self::Corrupt => "secret metadata is corrupt",
            Self::Unavailable => "secret metadata unavailable",
        };
        f.write_str(message)
    }
}

impl std::error::Error for SecretMetadataError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretBackendError {
    NotFound,
    Unavailable,
    /// The store exists but refused access (for example it is locked).
    Locked,
    /// The platform store failed at runtime (I/O or transient fault).
    Failure,
    /// Stored data or store format is malformed or ambiguous.
    Corrupt,
    Rejected,
}

impl fmt::Display for SecretBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "secret unavailable",
            Self::Unavailable => "credential backend unavailable",
            Self::Locked => "credential backend locked or access denied",
            Self::Failure => "credential backend failed",
            Self::Corrupt => "credential backend data invalid",
            Self::Rejected => "credential backend rejected the operation",
        };
        f.write_str(message)
    }
}

impl std::error::Error for SecretBackendError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretDeliveryError {
    Rejected,
}

impl fmt::Display for SecretDeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("secret delivery rejected")
    }
}

impl std::error::Error for SecretDeliveryError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretBrokerError {
    Metadata(SecretMetadataError),
    Backend(SecretBackendError),
    Unauthorized,
    BindingTargetMismatch,
    Delivery(SecretDeliveryError),
}

impl fmt::Display for SecretBrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => write!(f, "secret metadata failure: {error}"),
            Self::Backend(error) => write!(f, "secret backend failure: {error}"),
            Self::Unauthorized => f.write_str("secret release not authorized"),
            Self::BindingTargetMismatch => f.write_str("secret delivery binding mismatch"),
            Self::Delivery(error) => write!(f, "secret delivery failure: {error}"),
        }
    }
}

impl std::error::Error for SecretBrokerError {}

pub trait SecretMetadataStore {
    fn ensure_ref(&mut self, secret: &SecretRef) -> Result<(), SecretMetadataError>;
    fn has_ref(&self, secret: &SecretRef) -> Result<bool, SecretMetadataError>;
    fn put_binding(&mut self, binding: &SecretBinding) -> Result<(), SecretMetadataError>;
    fn get_binding(
        &self,
        id: &SecretBindingId,
    ) -> Result<Option<SecretBinding>, SecretMetadataError>;
}

pub trait SecretBackend: Send + Sync {
    fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError>;
    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError>;
    fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError>;
}

pub trait SecretDelivery {
    fn deliver(
        &mut self,
        target: &SecretDeliveryTarget,
        value: &[u8],
    ) -> Result<(), SecretDeliveryError>;
}

pub struct SecretsBroker<M, B> {
    metadata: M,
    backend: B,
}

impl<M, B> SecretsBroker<M, B>
where
    M: SecretMetadataStore,
    B: SecretBackend,
{
    pub fn new(metadata: M, backend: B) -> Self {
        Self { metadata, backend }
    }

    pub fn register_ref(&mut self, secret: &SecretRef) -> Result<(), SecretBrokerError> {
        self.metadata
            .ensure_ref(secret)
            .map_err(SecretBrokerError::Metadata)
    }

    pub fn bind(&mut self, binding: &SecretBinding) -> Result<(), SecretBrokerError> {
        if !self
            .metadata
            .has_ref(binding.secret())
            .map_err(SecretBrokerError::Metadata)?
        {
            return Err(SecretBrokerError::Metadata(SecretMetadataError::NotFound));
        }
        self.metadata
            .put_binding(binding)
            .map_err(SecretBrokerError::Metadata)
    }

    /// Resolves one opaque binding for trusted integration boundaries such as
    /// Security Audit. The returned value contains references/target identity
    /// only; secret material remains in the backend.
    pub fn binding(&self, id: &SecretBindingId) -> Result<SecretBinding, SecretBrokerError> {
        self.metadata
            .get_binding(id)
            .map_err(SecretBrokerError::Metadata)?
            .ok_or(SecretBrokerError::Metadata(SecretMetadataError::NotFound))
    }

    pub fn release(
        &self,
        binding_id: &SecretBindingId,
        target: &SecretDeliveryTarget,
        effective: &EffectiveCapabilities,
        delivery: &mut dyn SecretDelivery,
    ) -> Result<(), SecretBrokerError> {
        let binding = self
            .metadata
            .get_binding(binding_id)
            .map_err(SecretBrokerError::Metadata)?
            .ok_or(SecretBrokerError::Metadata(SecretMetadataError::NotFound))?;
        if binding.target() != target {
            return Err(SecretBrokerError::BindingTargetMismatch);
        }
        if !effective.permits(&binding.secret().capability()) {
            return Err(SecretBrokerError::Unauthorized);
        }

        let mut value = self
            .backend
            .load(binding.secret())
            .map_err(SecretBrokerError::Backend)?;
        let result = delivery
            .deliver(target, &value)
            .map_err(SecretBrokerError::Delivery);
        value.fill(0);
        result
    }

    pub fn into_parts(self) -> (M, B) {
        (self.metadata, self.backend)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;

    fn test_material() -> Vec<u8> {
        (0_u8..32)
            .map(|value| value.wrapping_mul(19).wrapping_add(7))
            .collect()
    }

    #[derive(Default)]
    struct MemoryMetadata {
        refs: Vec<SecretRef>,
        bindings: Vec<SecretBinding>,
    }

    impl SecretMetadataStore for MemoryMetadata {
        fn ensure_ref(&mut self, secret: &SecretRef) -> Result<(), SecretMetadataError> {
            if !self.refs.contains(secret) {
                self.refs.push(secret.clone());
            }
            Ok(())
        }

        fn has_ref(&self, secret: &SecretRef) -> Result<bool, SecretMetadataError> {
            Ok(self.refs.contains(secret))
        }

        fn put_binding(&mut self, binding: &SecretBinding) -> Result<(), SecretMetadataError> {
            if let Some(existing) = self.bindings.iter().find(|row| row.id() == binding.id()) {
                return if existing == binding {
                    Ok(())
                } else {
                    Err(SecretMetadataError::Conflict)
                };
            }
            self.bindings.push(binding.clone());
            Ok(())
        }

        fn get_binding(
            &self,
            id: &SecretBindingId,
        ) -> Result<Option<SecretBinding>, SecretMetadataError> {
            Ok(self.bindings.iter().find(|row| row.id() == id).cloned())
        }
    }

    #[derive(Clone, Default)]
    struct MemoryBackend {
        values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        loads: Arc<Mutex<usize>>,
    }

    impl SecretBackend for MemoryBackend {
        fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
            self.values
                .lock()
                .unwrap()
                .insert(secret.as_str().to_owned(), value.to_vec());
            Ok(())
        }

        fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
            *self.loads.lock().unwrap() += 1;
            self.values
                .lock()
                .unwrap()
                .get(secret.as_str())
                .cloned()
                .ok_or(SecretBackendError::NotFound)
        }

        fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
            self.values.lock().unwrap().remove(secret.as_str());
            Ok(())
        }
    }

    #[derive(Default)]
    struct CaptureDelivery {
        values: Vec<Vec<u8>>,
    }

    impl SecretDelivery for CaptureDelivery {
        fn deliver(
            &mut self,
            _target: &SecretDeliveryTarget,
            value: &[u8],
        ) -> Result<(), SecretDeliveryError> {
            self.values.push(value.to_vec());
            Ok(())
        }
    }

    fn fixture() -> (
        SecretsBroker<MemoryMetadata, MemoryBackend>,
        MemoryBackend,
        SecretRef,
        SecretBinding,
        Vec<u8>,
    ) {
        let backend = MemoryBackend::default();
        let observer = backend.clone();
        let secret = SecretRef::new("github-token").unwrap();
        let material = test_material();
        backend.store(&secret, &material).unwrap();
        let binding = SecretBinding::new(
            SecretBindingId::new("binding-1").unwrap(),
            secret.clone(),
            SecretDeliveryTarget::tool("github-api").unwrap(),
        );
        let mut broker = SecretsBroker::new(MemoryMetadata::default(), backend);
        broker.register_ref(&secret).unwrap();
        broker.bind(&binding).unwrap();
        (broker, observer, secret, binding, material)
    }

    #[test]
    fn authorized_exact_binding_releases_only_to_bound_target() {
        let (broker, _backend, secret, binding, material) = fixture();
        let effective = EffectiveCapabilities::new(vec![secret.capability()]);
        let mut delivery = CaptureDelivery::default();

        broker
            .release(binding.id(), binding.target(), &effective, &mut delivery)
            .unwrap();

        assert_eq!(delivery.values, [material]);
    }

    #[test]
    fn missing_secret_authority_fails_closed_before_backend_read() {
        let (broker, backend, _secret, binding, _material) = fixture();
        let mut delivery = CaptureDelivery::default();

        let error = broker
            .release(
                binding.id(),
                binding.target(),
                &EffectiveCapabilities::none(),
                &mut delivery,
            )
            .unwrap_err();

        assert_eq!(error, SecretBrokerError::Unauthorized);
        assert_eq!(*backend.loads.lock().unwrap(), 0);
        assert!(delivery.values.is_empty());
    }

    #[test]
    fn mismatched_delivery_target_fails_closed_before_backend_read() {
        let (broker, backend, secret, binding, _material) = fixture();
        let effective = EffectiveCapabilities::new(vec![secret.capability()]);
        let mut delivery = CaptureDelivery::default();

        let error = broker
            .release(
                binding.id(),
                &SecretDeliveryTarget::tool("another-tool").unwrap(),
                &effective,
                &mut delivery,
            )
            .unwrap_err();

        assert_eq!(error, SecretBrokerError::BindingTargetMismatch);
        assert_eq!(*backend.loads.lock().unwrap(), 0);
        assert!(delivery.values.is_empty());
    }

    #[test]
    fn broker_errors_never_contain_secret_material() {
        let material = test_material();
        let errors = [
            SecretBrokerError::Backend(SecretBackendError::Rejected),
            SecretBrokerError::Unauthorized,
            SecretBrokerError::BindingTargetMismatch,
            SecretBrokerError::Delivery(SecretDeliveryError::Rejected),
        ];
        for error in errors {
            let rendered = format!("{error:?} {error}");
            assert!(!rendered
                .as_bytes()
                .windows(material.len())
                .any(|window| window == material));
        }
    }
}
