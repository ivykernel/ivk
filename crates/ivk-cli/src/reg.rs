//! CLI-side registry access.
//!
//! Fail-open by design: the registry is bookkeeping, and a bookkeeping
//! problem must never block a workspace operation (an agent mid-task cannot
//! resolve a locked or corrupt db). On failure we warn on stderr and the
//! command proceeds without state tracking; `ivk doctor` surfaces the drift
//! later and `sync_from_disk` heals missing rows from the directory layout.

use std::path::Path;

use ivk_core::Registry;

/// Open (creating if needed) + backfill. For write paths (`new`, `ch`, `gc`).
pub fn open_synced(repo_root: &Path) -> Option<Registry> {
    match Registry::open_at_root(repo_root) {
        Ok(reg) => {
            if let Err(e) = reg.sync_from_disk(repo_root) {
                eprintln!("ivk: warning: registry sync failed: {}", e);
            }
            Some(reg)
        }
        Err(e) => {
            eprintln!("ivk: warning: registry unavailable: {}", e);
            None
        }
    }
}

/// Open + backfill only when `.ivk/` already exists. For read paths (`ls`,
/// `doctor`) that must not initialize anything in a non-ivk repo.
pub fn open_synced_if_present(repo_root: &Path) -> Option<Registry> {
    match Registry::open_if_present(repo_root) {
        Ok(Some(reg)) => {
            if let Err(e) = reg.sync_from_disk(repo_root) {
                eprintln!("ivk: warning: registry sync failed: {}", e);
            }
            Some(reg)
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("ivk: warning: registry unavailable: {}", e);
            None
        }
    }
}
