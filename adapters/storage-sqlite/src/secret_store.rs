//! TASK-0063 secret metadata persistence.
//!
//! Only opaque refs and exact delivery bindings are stored. Secret values are
//! owned by the external OS credential backend and cannot cross this module.

use rusqlite::{params, ErrorCode, OptionalExtension};
use secrets::{
    SecretBinding, SecretBindingId, SecretDeliveryKind, SecretDeliveryTarget, SecretMetadataError,
    SecretMetadataStore, SecretRef,
};

use crate::SqliteStore;

impl SecretMetadataStore for SqliteStore {
    fn ensure_ref(&mut self, secret: &SecretRef) -> Result<(), SecretMetadataError> {
        self.connection_mut()
            .execute(
                "INSERT OR IGNORE INTO secret_refs (ref_id) VALUES (?1)",
                params![secret.as_str()],
            )
            .map_err(map_sql)?;
        Ok(())
    }

    fn has_ref(&self, secret: &SecretRef) -> Result<bool, SecretMetadataError> {
        let found = self
            .connection()
            .query_row(
                "SELECT 1 FROM secret_refs WHERE ref_id = ?1",
                params![secret.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sql)?;
        Ok(found.is_some())
    }

    fn put_binding(&mut self, binding: &SecretBinding) -> Result<(), SecretMetadataError> {
        if !self.has_ref(binding.secret())? {
            return Err(SecretMetadataError::NotFound);
        }
        if let Some(existing) = self.get_binding(binding.id())? {
            return if &existing == binding {
                Ok(())
            } else {
                Err(SecretMetadataError::Conflict)
            };
        }
        self.connection_mut()
            .execute(
                "INSERT INTO secret_bindings (
                    binding_id, secret_ref_id, target_kind, target_id
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    binding.id().as_str(),
                    binding.secret().as_str(),
                    binding.target().kind().as_str(),
                    binding.target().id(),
                ],
            )
            .map_err(map_sql)?;
        Ok(())
    }

    fn get_binding(
        &self,
        id: &SecretBindingId,
    ) -> Result<Option<SecretBinding>, SecretMetadataError> {
        let row = self
            .connection()
            .query_row(
                "SELECT secret_ref_id, target_kind, target_id
                 FROM secret_bindings WHERE binding_id = ?1",
                params![id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sql)?;
        let Some((secret_ref, target_kind, target_id)) = row else {
            return Ok(None);
        };
        let secret = SecretRef::new(secret_ref).map_err(|_| SecretMetadataError::Corrupt)?;
        let target = match SecretDeliveryKind::parse(&target_kind)? {
            SecretDeliveryKind::Execution => SecretDeliveryTarget::execution(target_id),
            SecretDeliveryKind::Tool => SecretDeliveryTarget::tool(target_id),
        }
        .map_err(|_| SecretMetadataError::Corrupt)?;
        Ok(Some(SecretBinding::new(id.clone(), secret, target)))
    }
}

fn map_sql(error: rusqlite::Error) -> SecretMetadataError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::ConstraintViolation =>
        {
            SecretMetadataError::Conflict
        }
        _ => SecretMetadataError::Unavailable,
    }
}
