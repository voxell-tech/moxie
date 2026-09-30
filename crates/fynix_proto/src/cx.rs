//! What a view is built with: the world, the theme, where it hangs,
//! the rules in force there, and where live leaves are kept.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::marker::PhantomData;

use lenz::{Cursor, FieldId, FieldPath};
use typarena::type_table::TypeTable;

use crate::backend::Backend;
use crate::mounted::Mounted;
use crate::prop::Prop;
use crate::view::{Styled, View};

/// One rule restyling a `V`, with the theme in hand.
type Rule<V, T> = Box<dyn Fn(V, &T) -> V + Send + Sync>;

/// The rules restyling a `V` that one scope adds, in the order set.
type Rules<V, T> = Vec<Rule<V, T>>;

/// The fields of a `V` that one scope's path rules set, in the order
/// set.
struct Targets<V>(Vec<FieldId>, PhantomData<fn() -> V>);

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
    /// Set rules, one row per scope depth, one column per view kind.
    sets: TypeTable<usize>,
    /// Show rules, laid out like `sets`.
    shows: TypeTable<usize>,
    /// What the path rules among `sets` name, laid out like `sets`.
    targets: TypeTable<usize>,
    /// The innermost scope's depth. The root's is 0.
    depth: usize,
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
            sets: TypeTable::new(),
            shows: TypeTable::new(),
            targets: TypeTable::new(),
            depth: 0,
        }
    }

    pub fn theme(&self) -> &'a T {
        self.theme
    }

    /// Where live leaves are kept, to mount one.
    pub fn mounted(&mut self) -> &mut Mounted<B, T> {
        self.mounted
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
        self.sets.remove_row(&self.depth);
        self.shows.remove_row(&self.depth);
        self.targets.remove_row(&self.depth);
        self.depth -= 1;
        built
    }

    /// Restyles every `V` built from here to the end of the scope.
    /// A call-site value still beats it.
    pub fn set<V: Styled>(
        &mut self,
        rule: impl Fn(V, &T) -> V + Send + Sync + 'static,
    ) {
        push::<V, T>(&mut self.sets, self.depth, Box::new(rule));
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
        self.set::<P::Source>(move |mut view, theme| {
            if let Some(prop) = accessor.get_mut(&mut view) {
                *prop = Prop::Value(read(theme));
            }
            view
        });
        match self.targets.get_mut::<Targets<P::Source>>(&self.depth)
        {
            Some(targets) => targets.0.push(field),
            None => {
                self.targets.insert(
                    self.depth,
                    Targets::<P::Source>(vec![field], PhantomData),
                );
            }
        }
    }

    /// Which set rules in force for a `V` could decide `field`.
    pub fn trace<V: Styled>(&self, field: FieldId) -> Trace {
        let mut trace = Trace::default();
        for depth in 0..=self.depth {
            let named = self
                .targets
                .get::<Targets<V>>(&depth)
                .map_or(&[][..], |targets| &targets.0[..]);
            let rules = self
                .sets
                .get::<Rules<V, T>>(&depth)
                .map_or(0, Vec::len);
            trace.opaque += rules - named.len();
            if named.contains(&field) {
                trace.named.push(depth);
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
        push::<V, T>(&mut self.shows, self.depth, Box::new(rule));
    }

    /// `view` with the rules in force applied: set rules fill what its
    /// call site left unset, outer scopes first, then show rules
    /// transform the result.
    pub fn resolve<V: Styled>(&self, view: V) -> V {
        let theme = self.theme;
        let below = (0..=self.depth)
            .filter_map(|depth| self.sets.get::<Rules<V, T>>(&depth))
            .flatten()
            .fold(V::unset(), |below, rule| rule(below, theme));
        (0..=self.depth)
            .filter_map(|depth| self.shows.get::<Rules<V, T>>(&depth))
            .flatten()
            .fold(view.over(below), |view, rule| rule(view, theme))
    }
}

fn push<V: Styled, T: 'static>(
    table: &mut TypeTable<usize>,
    depth: usize,
    rule: Rule<V, T>,
) {
    match table.get_mut::<Rules<V, T>>(&depth) {
        Some(rules) => rules.push(rule),
        None => {
            table.insert(depth, vec![rule]);
        }
    }
}
