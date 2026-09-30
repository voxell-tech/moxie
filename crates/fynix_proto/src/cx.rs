use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::TypeId;

use lenz::{Cursor, FieldId, FieldPath};

use crate::backend::Backend;
use crate::mounted::{Group, Mounted};
use crate::prop::Prop;
use crate::rules::{RuleKind, ScopeEntry};
use crate::view::{Element, Styled, View};

/// One rule restyling a `V`, with the theme in hand.
type Rule<V, T> = Box<dyn Fn(V, &T) -> V + Send + Sync>;

/// Which set rules in force could decide a field, for telling where a
/// value came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    /// The scope depth of each path rule naming the field, outermost
    /// first. Unless the call site set it, the last one decided it.
    pub named: Vec<usize>,
    /// Closure rules in force for the same kind of view. Any of them
    /// could have set it too, and nothing can tell which.
    pub opaque: usize,
}

pub struct Cx<'a, B: Backend, T> {
    pub world: &'a mut B::World,
    theme: &'a T,
    mounted: &'a mut Mounted<B, T>,
    parent: Option<B::Node>,
    /// The rules in force, outermost first.
    entries: Vec<ScopeEntry>,
    /// The innermost scope's depth. The root's is 0.
    depth: usize,
    /// The group structural views built now belong to.
    owner: Option<Group>,
}

impl<B: Backend, T> Drop for Cx<'_, B, T> {
    fn drop(&mut self) {
        for entry in self.entries.drain(..) {
            self.mounted.rules.release(entry.key);
        }
    }
}

impl<'a, B: Backend, T: 'static> Cx<'a, B, T> {
    pub fn new(
        world: &'a mut B::World,
        theme: &'a T,
        mounted: &'a mut Mounted<B, T>,
    ) -> Self {
        Self {
            world,
            theme,
            mounted,
            parent: None,
            entries: Vec::new(),
            depth: 0,
            owner: None,
        }
    }

    /// A context with `capture` in force, for building again after the
    /// original is gone.
    pub(crate) fn seeded(
        world: &'a mut B::World,
        theme: &'a T,
        mounted: &'a mut Mounted<B, T>,
        capture: &[ScopeEntry],
        owner: Group,
    ) -> Self {
        for entry in capture {
            mounted.rules.retain(entry.key);
        }
        Self {
            world,
            theme,
            mounted,
            parent: None,
            entries: capture.to_vec(),
            depth: capture.last().map_or(0, |entry| entry.depth),
            owner: Some(owner),
        }
    }

    /// The rules in force now, each referred to once more.
    pub(crate) fn capture(&mut self) -> Vec<ScopeEntry> {
        for entry in &self.entries {
            self.mounted.rules.retain(entry.key);
        }
        self.entries.clone()
    }

    pub(crate) fn owner(&self) -> Option<Group> {
        self.owner
    }

    pub(crate) fn set_owner(&mut self, owner: Option<Group>) {
        self.owner = owner;
    }

    pub(crate) fn mounted(&mut self) -> &mut Mounted<B, T> {
        self.mounted
    }

    pub fn theme(&self) -> &'a T {
        self.theme
    }

    /// Keeps `element` in step with the world, as the one on `node`.
    pub fn mount<E: Element<B, T>>(
        &mut self,
        node: B::Node,
        element: E,
        snapshot: E::Snapshot,
    ) {
        self.mounted.mount(self.world, node, element, snapshot);
    }

    /// Where a view built now hangs. `None` at the root.
    pub fn parent(&self) -> Option<B::Node> {
        self.parent
    }

    /// A new, empty node where a view built now hangs.
    pub fn spawn(&mut self) -> B::Node {
        B::spawn(self.world, self.parent)
    }

    /// Builds `view` where a view built now hangs.
    pub fn build(&mut self, view: impl View<B, T>) -> B::Node {
        view.build(self)
    }

    /// Runs `build` with views hanging under `parent`.
    pub fn under<R>(
        &mut self,
        parent: B::Node,
        build: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let outer = self.parent.replace(parent);
        let built = build(self);
        self.parent = outer;
        built
    }

    /// Runs `build` in a scope of its own: rules it sets end with it.
    pub fn scope<R>(
        &mut self,
        build: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.depth += 1;
        let built = build(self);
        while let Some(entry) =
            self.entries.pop_if(|entry| entry.depth == self.depth)
        {
            self.mounted.rules.release(entry.key);
        }
        self.depth -= 1;
        built
    }

    /// Restyles every `V` built from here to the end of the scope.
    /// A call-site value still beats it.
    pub fn set<V: Styled>(
        &mut self,
        rule: impl Fn(V, &T) -> V + Send + Sync + 'static,
    ) {
        self.push::<V>(RuleKind::Set, None, Box::new(rule));
    }

    fn push<V: Styled>(
        &mut self,
        kind: RuleKind,
        field: Option<FieldId>,
        rule: Rule<V, T>,
    ) {
        let key = self.mounted.rules.insert(rule);
        self.entries.push(ScopeEntry {
            view: TypeId::of::<V>(),
            kind,
            key,
            field,
            depth: self.depth,
        });
    }

    /// The rules of `kind` for a `V`, outermost first.
    fn rules<V: Styled>(
        &self,
        kind: RuleKind,
    ) -> impl Iterator<Item = &Rule<V, T>> {
        self.entries
            .iter()
            .filter(move |entry| {
                entry.kind == kind && entry.view == TypeId::of::<V>()
            })
            .filter_map(|entry| {
                self.mounted.rules.get::<Rule<V, T>>(&entry.key)
            })
    }

    /// Sets the field `path` names to `value`, on every view `path`
    /// starts from, from here to the end of the scope. A set rule
    /// like [`set`](Self::set), but one that says which field it sets.
    ///
    /// The path may reach into a composite's own parts:
    /// `Card::cursor().title().size()` sets the size of a card's
    /// title, and of nothing else.
    pub fn set_field<P, X>(&mut self, path: Cursor<P>, value: X)
    where
        P: FieldPath<Target = Prop<B::World, X>>,
        P::Source: Styled,
        X: Clone + Send + Sync + 'static,
    {
        self.set_field_with(path, move |_| value.clone());
    }

    /// As [`set_field`](Self::set_field), with the value read from the
    /// theme.
    pub fn set_field_with<P, X>(
        &mut self,
        path: Cursor<P>,
        read: impl Fn(&T) -> X + Send + Sync + 'static,
    ) where
        P: FieldPath<Target = Prop<B::World, X>>,
        P::Source: Styled,
        X: Send + Sync + 'static,
    {
        let field = path.key();
        let accessor = path.accessor();
        self.push::<P::Source>(
            RuleKind::Set,
            Some(field),
            Box::new(move |mut view, theme| {
                if let Some(prop) = accessor.get_mut(&mut view) {
                    *prop = Prop::Value(read(theme));
                }
                view
            }),
        );
    }

    /// Which set rules in force for a `V` could decide `field`.
    pub fn trace<V: Styled>(&self, field: FieldId) -> Trace {
        let mut trace = Trace::default();
        let sets = self.entries.iter().filter(|entry| {
            entry.kind == RuleKind::Set
                && entry.view == TypeId::of::<V>()
        });
        for entry in sets {
            match entry.field {
                None => trace.opaque += 1,
                Some(named) if named == field => {
                    if trace.named.last() != Some(&entry.depth) {
                        trace.named.push(entry.depth);
                    }
                }
                Some(_) => {}
            }
        }
        trace
    }

    /// Transforms every `V` built from here to the end of the scope,
    /// after its call site, so unlike a set rule it wins over it.
    pub fn show<V: Styled>(
        &mut self,
        rule: impl Fn(V, &T) -> V + Send + Sync + 'static,
    ) {
        self.push::<V>(RuleKind::Show, None, Box::new(rule));
    }

    /// `view` with the rules in force applied: set rules fill what its
    /// call site left unset, outer scopes first, then show rules
    /// transform the result.
    pub fn resolve<V: Styled>(&self, view: V) -> V {
        let theme = self.theme;
        let below = self
            .rules::<V>(RuleKind::Set)
            .fold(V::unset(), |below, rule| rule(below, theme));
        self.rules::<V>(RuleKind::Show)
            .fold(view.over(below), |view, rule| rule(view, theme))
    }
}
