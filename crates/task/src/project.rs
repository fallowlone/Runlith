//! Minimal Project domain entity: identity plus name only.
//!
//! No organization, cloud, team, billing, or account semantics — those are
//! not required by G2.

use crate::error::{DomainError, DomainResult};
use crate::id::ProjectId;

/// A durable unit of work ownership grouping related tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    id: ProjectId,
    name: String,
}

impl Project {
    /// Creates a project with a validated id and non-blank name.
    pub fn new(id: ProjectId, name: impl Into<String>) -> DomainResult<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DomainError::input("project name must not be blank"));
        }
        Ok(Self {
            id,
            name: name.trim().to_owned(),
        })
    }

    /// Project identity.
    #[must_use]
    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    /// Human-readable project name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_holds_id_and_name() {
        let project = Project::new(ProjectId::new("P-1").unwrap(), "Runlith").unwrap();
        assert_eq!(project.id().as_str(), "P-1");
        assert_eq!(project.name(), "Runlith");
    }

    #[test]
    fn project_rejects_blank_name() {
        let id = ProjectId::new("P-1").unwrap();
        assert!(Project::new(id.clone(), "").is_err());
        assert!(Project::new(id, "   ").is_err());
    }
}
