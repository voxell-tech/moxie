//! The rules in force, stored once each and counted by whoever still
//! needs them.

use core::any::TypeId;

use hashbrown::HashMap;
use lenz::FieldId;
use typarena::type_pool::{PoolKey, TypePool};

/// Whether a rule fills what the call site left unset, or transforms
/// the result after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleKind {
    Set,
    Show,
}

/// One rule in a scope stack: the kind of view it restyles, where it is
/// stored, and how deep the scope that set it was.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeEntry {
    pub view: TypeId,
    pub kind: RuleKind,
    pub key: PoolKey,
    /// The field a path rule sets.
    pub field: Option<FieldId>,
    pub depth: usize,
}

/// Every rule any live scope or capture can still reach. A rule is kept
/// while its count says something refers to it.
#[derive(Default)]
pub struct RuleArena {
    pool: TypePool,
    counts: HashMap<PoolKey, u32>,
}

impl RuleArena {
    /// How many rules are stored.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Stores `rule`, referred to once.
    pub(crate) fn insert<R: Send + Sync + 'static>(
        &mut self,
        rule: R,
    ) -> PoolKey {
        let key = self.pool.insert(rule);
        self.counts.insert(key, 1);
        key
    }

    pub(crate) fn get<R: 'static>(
        &self,
        key: &PoolKey,
    ) -> Option<&R> {
        self.pool.get::<R>(key)
    }

    /// Adds a reference to the rule at `key`.
    pub(crate) fn retain(&mut self, key: PoolKey) {
        if let Some(count) = self.counts.get_mut(&key) {
            *count += 1;
        }
    }

    /// Drops a reference to the rule at `key`, and the rule with the
    /// last one.
    pub(crate) fn release(&mut self, key: PoolKey) {
        let Some(count) = self.counts.get_mut(&key) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.counts.remove(&key);
            self.pool.dyn_remove(&key);
        }
    }
}
