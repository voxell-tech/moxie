//! Elements whose props can change after they are built, kept in step
//! with the world.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::any::TypeId;
use core::marker::PhantomData;
use core::time::Duration;

use hashbrown::{HashMap, HashSet};
use typarena::type_table::TypeTable;

use crate::backend::Backend;
use crate::rules::RuleArena;
use crate::transition::Run;
use crate::view::Element;

/// A set of structural views built together, dropped together.
pub(crate) type Group = u32;

/// A view that builds and drops parts of the tree as the world
/// changes.
pub(crate) trait Structure<B: Backend, T>:
    Send + Sync
{
    /// Rebuilds what the world's changes call for.
    fn update(
        &mut self,
        id: Group,
        world: &mut B::World,
        theme: &T,
        mounted: &mut Mounted<B, T>,
    );

    /// Lets go of the rules it captured.
    fn release(&mut self, rules: &mut RuleArena);
}

/// One registered structural view.
struct Slot<B: Backend, T> {
    /// The group the slot was built in.
    parent: Option<Group>,
    container: B::Node,
    structure: Box<dyn Structure<B, T>>,
}

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
    /// Structural views by id, parents before the views they build.
    slots: BTreeMap<Group, Slot<B, T>>,
    /// The slot of each container node.
    containers: HashMap<B::Node, Group>,
    next_group: Group,
    pub(crate) rules: RuleArena,
}

impl<B: Backend, T> Default for Mounted<B, T> {
    fn default() -> Self {
        Self {
            table: TypeTable::new(),
            updates: Vec::new(),
            counts: Vec::new(),
            kinds: HashSet::new(),
            hooks: HashMap::new(),
            slots: BTreeMap::new(),
            containers: HashMap::new(),
            next_group: 0,
            rules: RuleArena::default(),
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

    /// Drops the element on `node`, and the structural view whose
    /// container it is.
    pub fn unmount(&mut self, node: B::Node) {
        if let Some(hooks) = self.hooks.remove(&node) {
            (hooks.remove)(&mut self.table, node);
        }
        if let Some(id) = self.containers.remove(&node) {
            if let Some(slot) = self.slots.remove(&id) {
                self.forget(slot);
            }
            self.drop_group(id);
        }
    }

    /// A group for structural views built together.
    pub(crate) fn new_group(&mut self) -> Group {
        self.next_group += 1;
        self.next_group
    }

    /// Registers `structure` as the one with `container`, under the
    /// group `id` that `new_group` gave.
    pub(crate) fn register(
        &mut self,
        id: Group,
        parent: Option<Group>,
        container: B::Node,
        structure: Box<dyn Structure<B, T>>,
    ) {
        self.containers.insert(container, id);
        self.slots.insert(
            id,
            Slot {
                parent,
                container,
                structure,
            },
        );
    }

    /// Drops every structural view built in `group`, and in the
    /// groups those made.
    pub(crate) fn drop_group(&mut self, group: Group) {
        let mut dead = vec![group];
        let mut next = 0;
        while let Some(&group) = dead.get(next) {
            next += 1;
            let built = self
                .slots
                .iter()
                .filter(|(_, slot)| slot.parent == Some(group))
                .map(|(&id, _)| id)
                .collect::<Vec<_>>();
            for id in built {
                if let Some(slot) = self.slots.remove(&id) {
                    self.forget(slot);
                }
                dead.push(id);
            }
        }
    }

    fn forget(&mut self, mut slot: Slot<B, T>) {
        self.containers.remove(&slot.container);
        slot.structure.release(&mut self.rules);
    }

    /// Rebuilds what the world's changes call for, in every `keyed`
    /// and `each` view. Views they build are checked from the next
    /// update on.
    pub fn update_structure(
        &mut self,
        world: &mut B::World,
        theme: &T,
    ) {
        let ids = self.slots.keys().copied().collect::<Vec<_>>();
        for id in ids {
            // Gone when the rebuild of a view before it dropped it.
            let Some(mut slot) = self.slots.remove(&id) else {
                continue;
            };
            slot.structure.update(id, world, theme, self);
            self.slots.insert(id, slot);
        }
    }

    /// Writes what changed to every mounted element that reports a
    /// change or is marked dirty, and keeps every transition under
    /// way advancing.
    pub fn update_elements(
        &mut self,
        world: &mut B::World,
        theme: &T,
        tick: Tick,
    ) {
        for update in &self.updates {
            update(&mut self.table, world, theme, tick);
        }
    }

    /// The rules stored for the scopes and captures alive.
    pub fn rules(&self) -> &RuleArena {
        &self.rules
    }

    /// How many `keyed` and `each` views are registered.
    pub fn structure_len(&self) -> usize {
        self.slots.len()
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
