//! Leaves whose props can change after they are built, kept in step
//! with the world.

use core::marker::PhantomData;

use bevy::prelude::*;

use crate::Theme;
use crate::transition::{ReducedMotion, Run};
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
    /// Where the values are heading.
    target: L::Snapshot,
    /// What is written on the node.
    shown: L::Snapshot,
    run: Option<Run<L::Snapshot>>,
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
        let reduced = world
            .get_resource::<ReducedMotion>()
            .is_some_and(|reduced| reduced.0);
        let mut now = self.leaf.snapshot(world, theme);
        self.leaf.adjust(&mut now, world, self.node, theme);

        if now != self.target {
            self.target = now;
            self.run = self
                .leaf
                .tween(theme)
                .filter(|tween| {
                    !reduced && !tween.curve.duration.is_zero()
                })
                .map(|tween| Run::new(self.shown.clone(), tween));
            if self.run.is_none() {
                self.shown = self.target.clone();
                L::write(&self.shown, world, self.node);
            }
        }

        if let Some(run) = &mut self.run {
            let delta = world.resource::<Time>().delta();
            let next = match run.advance(delta, &self.target, reduced)
            {
                Some(next) => next,
                None => {
                    self.run = None;
                    self.target.clone()
                }
            };
            if next != self.shown {
                L::write(&next, world, self.node);
                self.shown = next;
            }
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
        snapshot: L::Snapshot,
    ) {
        self.0.push(Box::new(Mount {
            node,
            leaf,
            target: snapshot.clone(),
            shown: snapshot,
            run: None,
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
