//! Live-observed worktree status (never persisted as authority).
//!
//! Status is derived from Git on every inspection. `dirty != safe to
//! delete`: any dirty shape blocks normal cleanup (`CleanupUnsafe`).

/// Observed worktree condition, Git-authoritative at inspection time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorktreeStatus {
    /// No tracked modifications, no untracked files.
    Clean,
    /// Tracked files modified, staged, or both.
    DirtyTracked,
    /// Untracked files present (with or without tracked changes).
    DirtyUntracked,
    /// Registered worktree path absent from the filesystem.
    Missing,
    /// Present but unusable (e.g. unexpected detached HEAD, corrupt
    /// metadata, path escapes the managed root).
    Invalid,
}

impl WorktreeStatus {
    /// Whether ordinary (non-destructive-authorized) cleanup may proceed.
    /// Only a `Clean` worktree is silently cleanable; every other
    /// observable shape fails closed.
    #[must_use]
    pub const fn allows_normal_cleanup(self) -> bool {
        matches!(self, Self::Clean)
    }

    /// Whether the shape counts as dirty for safety purposes. Untracked
    /// files are dirt: they may be task evidence and are never silently
    /// omitted from cleanup safety.
    #[must_use]
    pub const fn is_dirty(self) -> bool {
        matches!(self, Self::DirtyTracked | Self::DirtyUntracked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clean_allows_normal_cleanup() {
        assert!(WorktreeStatus::Clean.allows_normal_cleanup());
        for status in [
            WorktreeStatus::DirtyTracked,
            WorktreeStatus::DirtyUntracked,
            WorktreeStatus::Missing,
            WorktreeStatus::Invalid,
        ] {
            assert!(!status.allows_normal_cleanup(), "{status:?}");
        }
    }

    #[test]
    fn untracked_files_count_as_dirty() {
        assert!(WorktreeStatus::DirtyUntracked.is_dirty());
        assert!(WorktreeStatus::DirtyTracked.is_dirty());
        assert!(!WorktreeStatus::Clean.is_dirty());
    }
}
