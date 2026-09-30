//! Elements whose props can change after they are built, kept in step
//! with the world.

use alloc::vec::Vec;
use core::any::TypeId;
use core::marker::PhantomData;
use core::time::Duration;

use hashbrown::{HashMap, HashSet};
use typarena::type_table::TypeTable;

use crate::backend::Backend;
use crate::transition::Run;
use crate::view::Element;

/// What one update runs with.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    /// Time since the last update, what transitions advance by.
    pub delta: Duration,
    /// Whether every transition finishes at once.
    pub reduced_motion: bool,
}

/// Brings every mounted element of one kind up to date.
type UpdateFn<B, T> = fn(
    &mut TypeTable<<B as Backend>::Node>,
    &mut <B as Backend>::World,
    &T,
    Tick,
);

/// How many elements of one kind are mounted.
type CountFn<B> = fn(&TypeTable<<B as Backend>::Node>) -> usize;

/// Acts on the mounted element of one kind on a node, without naming
/// the kind.
type NodeFn<B> =
    fn(&mut TypeTable<<B as Backend>::Node>, <B as Backend>::Node);

/// What [`Mounted`] does to a node's element, for the element's kind.
struct Hooks<B: Backend> {
    mark: NodeFn<B>,
    remove: NodeFn<B>,
}

impl<B: Backend> Clone for Hooks<B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B: Backend> Copy for Hooks<B> {}

/// Every mounted element built with the theme `T`: one column per kind of
/// element, keyed by its node, and one update per kind to walk it.
pub struct Mounted<B: Backend, T> {
    table: TypeTable<B::Node>,
    updates: Vec<UpdateFn<B, T>>,
    counts: Vec<CountFn<B>>,
    kinds: HashSet<TypeId>,
    hooks: HashMap<B::Node, Hooks<B>>,
}

impl<B: Backend, T> Default for Mounted<B, T> {
    fn default() -> Self {
        Self {
            table: TypeTable::new(),
            updates: Vec::new(),
            counts: Vec::new(),
            kinds: HashSet::new(),
            hooks: HashMap::new(),
        }
    }
}

impl<B: Backend, T: 'static> Mounted<B, T> {
    /// Keeps `element` in step with the world as the one on `node`.
    pub fn mount<E: Element<B, T>>(
        &mut self,
        world: &mut B::World,
        node: B::Node,
        mut element: E,
        snapshot: E::Snapshot,
    ) {
        if self.kinds.insert(TypeId::of::<E>()) {
            self.updates.push(update_kind::<B, T, E>);
            self.counts.push(|table| table.len::<Mount<B, T, E>>());
        }
        self.hooks.insert(
            node,
            Hooks {
                mark: mark::<B, T, E>,
                remove: remove::<B, T, E>,
            },
        );
        // Checks often fire on their first call, which the snapshot
        // just taken already covers.
        element.changed(world);
        B::on_mount(world, node);
        element.on_mounted(world, node);
        self.table.insert(
            node,
            Mount::<B, T, E> {
                element,
                target: snapshot.clone(),
                shown: snapshot,
                run: None,
                dirty: false,
                marker: PhantomData,
            },
        );
    }

    /// Makes the element on `node` re-read at the next update, whatever
    /// its checks say.
    pub fn mark_dirty(&mut self, node: B::Node) {
        if let Some(hooks) = self.hooks.get(&node) {
            (hooks.mark)(&mut self.table, node);
        }
    }

    /// Drops the element on `node`.
    pub fn unmount(&mut self, node: B::Node) {
        if let Some(hooks) = self.hooks.remove(&node) {
            (hooks.remove)(&mut self.table, node);
        }
    }

    /// Writes what changed to every mounted element that reports a
    /// change or is marked dirty, and keeps every transition under
    /// way advancing.
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

struct Mount<B: Backend, T, E: Element<B, T>> {
    element: E,
    /// Where the written values are heading.
    target: E::Snapshot,
    /// What is written on the node.
    shown: E::Snapshot,
    run: Option<Run<E::Snapshot>>,
    /// Whether to re-read at the next update whatever the checks say.
    dirty: bool,
    marker: PhantomData<fn() -> (B, T)>,
}

fn update_kind<B: Backend, T: 'static, E: Element<B, T>>(
    table: &mut TypeTable<B::Node>,
    world: &mut B::World,
    theme: &T,
    tick: Tick,
) {
    for (&node, mount) in table.iter_mut::<Mount<B, T, E>>() {
        mount.update(world, node, theme, tick);
    }
}

fn mark<B: Backend, T: 'static, E: Element<B, T>>(
    table: &mut TypeTable<B::Node>,
    node: B::Node,
) {
    if let Some(mount) = table.get_mut::<Mount<B, T, E>>(&node) {
        mount.dirty = true;
    }
}

fn remove<B: Backend, T: 'static, E: Element<B, T>>(
    table: &mut TypeTable<B::Node>,
    node: B::Node,
) {
    table.remove::<Mount<B, T, E>>(&node);
}

impl<B: Backend, T, E: Element<B, T>> Mount<B, T, E> {
    fn update(
        &mut self,
        world: &mut B::World,
        node: B::Node,
        theme: &T,
        tick: Tick,
    ) {
        // Not `||`: every check must run, each keeps its own memory.
        let stale = self.element.changed(world) | self.dirty;
        self.dirty = false;
        if !stale && self.run.is_none() {
            return;
        }

        if stale {
            let mut now = self.element.snapshot(world, theme);
            self.element.adjust(&mut now, world, node, theme);

            if now != self.target {
                self.target = now;
                self.run = self
                    .element
                    .tween(theme)
                    .filter(|tween| {
                        !tick.reduced_motion
                            && !tween.curve.duration.is_zero()
                    })
                    .map(|tween| Run::new(self.shown.clone(), tween));
            }
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
            E::write(&next, world, node);
            self.shown = next;
        }
    }
}
