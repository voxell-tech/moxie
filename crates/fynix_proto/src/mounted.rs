//! Leaves whose props can change after they are built, kept in step
//! with the world.

use core::any::TypeId;
use core::marker::PhantomData;
use core::time::Duration;

use hashbrown::HashSet;
use typarena::type_table::TypeTable;

use crate::backend::Backend;
use crate::transition::Run;
use crate::view::Leaf;

/// What one update runs with.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    /// Time since the last update, what transitions advance by.
    pub delta: Duration,
    /// Whether every transition finishes at once.
    pub reduced_motion: bool,
}

/// Brings every mounted leaf of one kind up to date.
type UpdateFn<B, T> = fn(
    &mut TypeTable<<B as Backend>::Node>,
    &mut <B as Backend>::World,
    &T,
    Tick,
);

/// How many leaves of one kind are mounted.
type CountFn<B> = fn(&TypeTable<<B as Backend>::Node>) -> usize;

/// Every mounted leaf built with the theme `T`: one column per kind of
/// leaf, keyed by its node, and one update per kind to walk it.
pub struct Mounted<B: Backend, T> {
    table: TypeTable<B::Node>,
    updates: Vec<UpdateFn<B, T>>,
    counts: Vec<CountFn<B>>,
    kinds: HashSet<TypeId>,
}

impl<B: Backend, T> Default for Mounted<B, T> {
    fn default() -> Self {
        Self {
            table: TypeTable::new(),
            updates: Vec::new(),
            counts: Vec::new(),
            kinds: HashSet::new(),
        }
    }
}

impl<B: Backend, T: 'static> Mounted<B, T> {
    pub fn mount<L: Leaf<B, T>>(
        &mut self,
        node: B::Node,
        leaf: L,
        snapshot: L::Snapshot,
    ) {
        if self.kinds.insert(TypeId::of::<L>()) {
            self.updates.push(update_kind::<B, T, L>);
            self.counts.push(|table| table.len::<Mount<B, T, L>>());
        }
        self.table.insert(
            node,
            Mount::<B, T, L> {
                leaf,
                target: snapshot.clone(),
                shown: snapshot,
                run: None,
                marker: PhantomData,
            },
        );
    }

    /// Re-reads every mounted leaf and writes what changed, dropping
    /// any whose node is gone.
    pub fn update(
        &mut self,
        world: &mut B::World,
        theme: &T,
        tick: Tick,
    ) {
        for update in &self.updates {
            update(&mut self.table, world, theme, tick);
        }
    }

    pub fn len(&self) -> usize {
        self.counts.iter().map(|count| count(&self.table)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct Mount<B: Backend, T, L: Leaf<B, T>> {
    leaf: L,
    /// Where the written values are heading.
    target: L::Snapshot,
    /// What is written on the node.
    shown: L::Snapshot,
    run: Option<Run<L::Snapshot>>,
    marker: PhantomData<fn() -> (B, T)>,
}

fn update_kind<B: Backend, T: 'static, L: Leaf<B, T>>(
    table: &mut TypeTable<B::Node>,
    world: &mut B::World,
    theme: &T,
    tick: Tick,
) {
    let mut gone = Vec::new();
    for (&node, mount) in table.iter_mut::<Mount<B, T, L>>() {
        if B::exists(world, node) {
            mount.update(world, node, theme, tick);
        } else {
            gone.push(node);
        }
    }
    for node in gone {
        table.remove::<Mount<B, T, L>>(&node);
    }
}

impl<B: Backend, T, L: Leaf<B, T>> Mount<B, T, L> {
    fn update(
        &mut self,
        world: &mut B::World,
        node: B::Node,
        theme: &T,
        tick: Tick,
    ) {
        let mut now = self.leaf.snapshot(world, theme);
        self.leaf.adjust(&mut now, world, node, theme);

        if now != self.target {
            self.target = now;
            self.run = self
                .leaf
                .tween(theme)
                .filter(|tween| {
                    !tick.reduced_motion
                        && !tween.curve.duration.is_zero()
                })
                .map(|tween| Run::new(self.shown.clone(), tween));
        }
        if tick.reduced_motion {
            self.run = None;
        }

        let next = match &mut self.run {
            Some(run) => {
                match run.advance(tick.delta, &self.target) {
                    Some(next) => next,
                    None => {
                        self.run = None;
                        self.target.clone()
                    }
                }
            }
            None => self.target.clone(),
        };
        if next != self.shown {
            L::write(&next, world, node);
            self.shown = next;
        }
    }
}
