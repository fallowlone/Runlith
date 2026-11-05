-- ADR-0051: explicit workspace-change intent on the canonical Task.
-- Default 1 preserves the prior behavior for every existing task: a task
-- with a declared write scope was always required to change its worktree.
ALTER TABLE tasks ADD COLUMN requires_workspace_change INTEGER NOT NULL DEFAULT 1
    CHECK (requires_workspace_change IN (0, 1));
