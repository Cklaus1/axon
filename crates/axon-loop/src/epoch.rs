//! B258 (epoch half) — the per-scope authority epoch.
//!
//! The epoch is not a separate counter that could drift from the pointer: it
//! IS `pointer.json`'s `epoch`, advanced only by a successful fenced
//! [`crate::pointer::transition`]. A never-transitioned scope is epoch 0.
//! Effects that must "recheck the current epoch" compare against
//! [`current`]; a record carrying any other epoch is stale.

use crate::error::{LoopError, Result};
use crate::store::Store;
use axon_loop_contracts::{AuthorityEpoch, Scope};

/// The scope's current authority epoch (read under the scope lock, after
/// crash recovery, so it never reports a half-applied transition).
pub fn current(store: &Store, scope: &Scope) -> Result<AuthorityEpoch> {
    Ok(crate::pointer::load(store, scope)?.epoch)
}

/// Refuse unless `claimed` is the scope's current epoch.
pub fn require_current(store: &Store, scope: &Scope, claimed: AuthorityEpoch) -> Result<()> {
    let now = current(store, scope)?;
    if now != claimed {
        return Err(LoopError::Conflict(format!(
            "stale authority epoch {} (current {})",
            claimed.get(),
            now.get()
        )));
    }
    Ok(())
}
