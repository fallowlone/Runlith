//! How one provider launch authenticates (providers.md §9a, ADR-0039
//! decision 7).
//!
//! The mode is decided once per launch context from detection and the
//! platform's confinement verdict. In brokered mode the operator's host
//! login is never probed and never used as a fallback: the vault credential
//! is checked before any G6 work and released through the Secrets Broker
//! into the late-bound child secret slot after authorization.

use std::ffi::OsStr;

use app_services::{G6SecurityService, PrivilegedOperation, SystemClock};
use policy::{Capability, EffectiveCapabilities};
use runner::ChildSecretEnvironment;
use secrets::{SecretBinding, SecretBindingId, SecretDeliveryTarget, SecretRef, SecretsBroker};

use crate::execution::{ConfinementAuthVerdict, PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED};
use crate::vault::{self, VaultHandle, PROVIDER_CREDENTIAL_ENTRY};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderCredentialMode {
    /// Not the real, compatible CLI (fixture, unrecognized binary): it has
    /// not claimed to be the working provider and owns no credential (§9a).
    NotRequired,
    /// Real CLI where no mandatory confinement is installed: the operator's
    /// host login, observed by the bounded probe (§9a).
    HostLogin,
    /// Real CLI inside confinement measured compatible: the brokered vault
    /// credential. No host probe, no fallback.
    Brokered,
}

/// Credential source bound into one launch's security context.
#[derive(Clone, Debug)]
pub(crate) struct ProviderCredential {
    mode: ProviderCredentialMode,
    vault: Option<VaultHandle>,
    now_s: fn() -> u64,
}

impl ProviderCredential {
    /// Decides the mode for `executable`. A real CLI under a confinement
    /// measured unsupported refuses here, before any provider work.
    pub(crate) fn resolve(
        executable: &OsStr,
        verdict: ConfinementAuthVerdict,
        vault: Option<VaultHandle>,
        now_s: fn() -> u64,
    ) -> Result<Self, String> {
        let mode = if provider_claude::detect(executable).status
            != provider_claude::ProviderAvailability::Available
        {
            ProviderCredentialMode::NotRequired
        } else {
            match verdict {
                ConfinementAuthVerdict::NoMandatoryConfinement => ProviderCredentialMode::HostLogin,
                ConfinementAuthVerdict::Compatible => ProviderCredentialMode::Brokered,
                ConfinementAuthVerdict::Unsupported(reason) => {
                    return Err(format!("{PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED}: {reason}"));
                }
            }
        };
        Ok(Self { mode, vault, now_s })
    }

    /// The broker capability a brokered launch must hold; `None` otherwise.
    pub(crate) fn secret_capability(&self) -> Result<Option<Capability>, String> {
        if self.mode != ProviderCredentialMode::Brokered {
            return Ok(None);
        }
        Capability::secret(PROVIDER_CREDENTIAL_ENTRY)
            .map(Some)
            .map_err(|error| format!("provider credential scope: {error}"))
    }

    /// Pre-launch precondition: the host login for [`ProviderCredentialMode::HostLogin`],
    /// the vault credential for [`ProviderCredentialMode::Brokered`].
    pub(crate) fn preflight(&self, executable: &OsStr) -> Result<(), String> {
        match self.mode {
            ProviderCredentialMode::NotRequired => Ok(()),
            ProviderCredentialMode::HostLogin => {
                // Scope limit: this observes the operator's state on the
                // host, not a confined child's ability to reach it.
                let authentication = provider_claude::probe_authentication(executable);
                if authentication.state.permits_launch() {
                    Ok(())
                } else {
                    Err(authentication.refusal())
                }
            }
            ProviderCredentialMode::Brokered => {
                vault::check_provider_credential(self.vault.as_ref(), (self.now_s)())
                    .map_err(|refusal| refusal.message().to_owned())
            }
        }
    }

    /// Releases the brokered credential for one authorized launch through
    /// the audited G6 path. Other modes release nothing.
    pub(crate) fn release(
        &self,
        security: &mut G6SecurityService<SystemClock>,
        operation: &PrivilegedOperation,
        effective: &EffectiveCapabilities,
    ) -> Result<Option<ChildSecretEnvironment>, String> {
        if self.mode != ProviderCredentialMode::Brokered {
            return Ok(None);
        }
        let handle = self
            .vault
            .as_ref()
            .ok_or_else(|| vault::CredentialRefusal::VaultAbsent.message().to_owned())?;
        release_from_vault(handle, security, operation, effective).map(Some)
    }
}

/// One audited Secrets Broker release: a launch-scoped binding of the
/// provider credential to this operation, delivered into the child slot.
fn release_from_vault(
    handle: &VaultHandle,
    security: &mut G6SecurityService<SystemClock>,
    operation: &PrivilegedOperation,
    effective: &EffectiveCapabilities,
) -> Result<ChildSecretEnvironment, String> {
    let refused =
        |error: &dyn std::fmt::Display| format!("provider credential release refused: {error}");
    let secret = SecretRef::new(PROVIDER_CREDENTIAL_ENTRY).map_err(|error| refused(&error))?;
    let target = SecretDeliveryTarget::execution(operation.operation_id.clone())
        .map_err(|error| refused(&error))?;
    let binding_id =
        SecretBindingId::new(format!("provider-credential:{}", operation.operation_id))
            .map_err(|error| refused(&error))?;
    let mut broker = SecretsBroker::new(
        vault::LaunchSecretMetadata::default(),
        vault::VaultCredentialBackend(handle.clone()),
    );
    broker
        .register_ref(&secret)
        .map_err(|error| refused(&error))?;
    broker
        .bind(&SecretBinding::new(
            binding_id.clone(),
            secret,
            target.clone(),
        ))
        .map_err(|error| refused(&error))?;
    let mut delivery = vault::ProviderCredentialDelivery::default();
    security
        .release_secret(
            operation,
            &broker,
            &binding_id,
            &target,
            effective,
            &mut delivery,
        )
        .map_err(|error| refused(&error))?;
    Ok(delivery.0)
}
