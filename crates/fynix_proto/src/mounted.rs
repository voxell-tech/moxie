//! Leaves whose props can change after they are built, kept in step
//! with the world.

use core::marker::PhantomData;

use bevy::prelude::*;

use crate::Theme;
use crate::view::Leaf;

/// One mounted leaf, its kind erased.
trait Live<T>: Send + Sync {
    /// Re-reads the leaf's props and writes what changed. `false` once
    /// its node is gone, to be dropped.
    fn update(&mut self, world: &mut World, theme: &T) -> bool;
}

struct Mount<L: Leaf<T>, T> {
    node: Entity,
    leaf: L,
    last: L::Snapshot,
    theme: PhantomData<fn() -> T>,
}

impl<T, L: Leaf<T>> Live<T> for Mount<L, T>
where
    T: Send + Sync,
{
    fn update(&mut self, world: &mut World, theme: &T) -> bool {
        if world.get_entity(self.node).is_err() {
            return false;
        }
        let now = self.leaf.snapshot(world, theme);
        if now != self.last {
            L::write(&now, world, self.node);
            self.last = now;
        }
        true
    }
}

/// Every mounted leaf built with the theme `T`.
#[derive(Resource)]
pub struct Mounted<T>(Vec<Box<dyn Live<T>>>);

impl<T> Default for Mounted<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T: Send + Sync + 'static> Mounted<T> {
    pub(crate) fn mount<L: Leaf<T>>(
        &mut self,
        node: Entity,
        leaf: L,
        last: L::Snapshot,
    ) {
        self.0.push(Box::new(Mount {
            node,
            leaf,
            last,
            theme: PhantomData,
        }));
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Brings every mounted leaf up to date, dropping any whose node is
/// gone.
pub(crate) fn update<T: Send + Sync + 'static>(world: &mut World) {
    let mut mounted =
        core::mem::take(&mut world.resource_mut::<Mounted<T>>().0);
    world.resource_scope::<Theme<T>, _>(|world, theme| {
        mounted.retain_mut(|live| live.update(world, &theme.0));
    });
    // Leaves mounted while updating (none today) come after.
    let mut resource = world.resource_mut::<Mounted<T>>();
    mounted.append(&mut resource.0);
    resource.0 = mounted;
}
