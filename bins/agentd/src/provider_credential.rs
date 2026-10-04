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

/// Claude Code version of the ADR-0039 brokered measurement (Addendum C).
const MEASURED_CLI_VERSION: (u64, u64, u64) = (2, 1, 289);

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
    /// Brokered launches on an unmeasured CLI version warn (owner choice a2).
    version_note: Option<String>,
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
        let detection = provider_claude::detect(executable);
        let mode = if detection.status != provider_claude::ProviderAvailability::Available {
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
        let version_note = (mode == ProviderCredentialMode::Brokered)
            .then(|| unmeasured_version_note(detection.version.as_ref()))
            .flatten();
        Ok(Self {
            mode,
            vault,
            now_s,
            version_note,
        })
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
                if let Some(note) = &self.version_note {
                    eprintln!("agentd: warning: {note}");
                }
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

    /// Records a provider refusal of the released brokered credential, read
    /// from the adapter-rendered failure (ADR-0039 decision 7).
    pub(crate) fn observe_failure(&self, rendered: &str) {
        if self.mode == ProviderCredentialMode::Brokered
            && provider_claude::is_credential_rejected(rendered)
        {
            vault::mark_provider_credential_rejected(self.vault.as_ref());
        }
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

/// Warning for a brokered launch on a Claude Code version other than the
/// one measured for ADR-0039 (owner choice a2: warn, do not refuse).
fn unmeasured_version_note(version: Option<&provider_claude::CliVersion>) -> Option<String> {
    let (major, minor, patch) = MEASURED_CLI_VERSION;
    let observed = match version {
        Some(version) if (version.major, version.minor, version.patch) == MEASURED_CLI_VERSION => {
            return None;
        }
        Some(version) => format!("{}.{}.{}", version.major, version.minor, version.patch),
        None => "unknown".to_owned(),
    };
    Some(format!(
        "claude CLI {observed} differs from {major}.{minor}.{patch}, the version measured for \
         brokered confinement (ADR-0039); repeat the measurement"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(patch: u64) -> provider_claude::CliVersion {
        provider_claude::CliVersion {
            major: 2,
            minor: 1,
            patch,
        }
    }

    #[test]
    fn only_the_measured_cli_version_launches_without_a_warning() {
        assert_eq!(unmeasured_version_note(Some(&version(289))), None);
        let note = unmeasured_version_note(Some(&version(290))).expect("warning");
        assert!(
            note.contains("2.1.290") && note.contains("2.1.289"),
            "{note}"
        );
        assert!(unmeasured_version_note(None).is_some());
    }
}
