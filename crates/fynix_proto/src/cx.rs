//! What a view is built with: the world, the theme, where it hangs,
//! and the rules in force there.

use core::any::{Any, TypeId};
use std::collections::HashMap;

use crate::backend::Backend;
use crate::view::{Styled, View};

/// A set or show rule for views of one kind, erased so rules for every
/// kind share a table.
type ErasedRule = Box<dyn Any + Send + Sync>;

/// A rule restyling a `V`, with the theme in hand.
type Rule<V, T> = Box<dyn Fn(V, &T) -> V + Send + Sync>;

/// The rules one scope adds, by the kind of view they restyle.
#[derive(Default)]
struct Scope {
    sets: HashMap<TypeId, Vec<ErasedRule>>,
    shows: HashMap<TypeId, Vec<ErasedRule>>,
}

pub struct Cx<'a, B: Backend, T> {
    pub world: &'a mut B::World,
    theme: &'a T,
    parent: Option<B::Node>,
    /// Outermost first. Never empty: the first is the root's.
    scopes: Vec<Scope>,
}

impl<'a, B: Backend, T: 'static> Cx<'a, B, T> {
    pub fn new(world: &'a mut B::World, theme: &'a T) -> Self {
        Self {
            world,
            theme,
            parent: None,
            scopes: vec![Scope::default()],
        }
    }

    pub fn theme(&self) -> &'a T {
        self.theme
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
        self.scopes.push(Scope::default());
        let built = build(self);
        self.scopes.pop();
        built
    }

    /// Restyles every `V` built from here to the end of the scope.
    /// A call-site value still beats it.
    pub fn set<V: Styled>(
        &mut self,
        rule: impl Fn(V, &T) -> V + Send + Sync + 'static,
    ) {
        let rule: Rule<V, T> = Box::new(rule);
        self.innermost()
            .sets
            .entry(TypeId::of::<V>())
            .or_default()
            .push(Box::new(rule));
    }

    /// Transforms every `V` built from here to the end of the scope,
    /// after its call site. Unlike a set rule, it wins over the call
    /// site.
    pub fn show<V: Styled>(
        &mut self,
        rule: impl Fn(V, &T) -> V + Send + Sync + 'static,
    ) {
        let rule: Rule<V, T> = Box::new(rule);
        self.innermost()
            .shows
            .entry(TypeId::of::<V>())
            .or_default()
            .push(Box::new(rule));
    }

    /// `view` with the rules in force applied: set rules fill what its
    /// call site left unset, outer scopes first, then show rules
    /// transform the result.
    pub fn resolve<V: Styled>(&self, view: V) -> V {
        let theme = self.theme;
        let below = self
            .scopes
            .iter()
            .flat_map(|scope| rules_for::<V, T>(&scope.sets))
            .fold(V::unset(), |below, rule| rule(below, theme));
        self.scopes
            .iter()
            .flat_map(|scope| rules_for::<V, T>(&scope.shows))
            .fold(view.over(below), |view, rule| rule(view, theme))
    }

    fn innermost(&mut self) -> &mut Scope {
        self.scopes
            .last_mut()
            .expect("the root scope is never popped")
    }
}

/// The rules in `table` for views of kind `V`, in the order they were
/// set.
fn rules_for<V: 'static, T: 'static>(
    table: &HashMap<TypeId, Vec<ErasedRule>>,
) -> impl Iterator<Item = &Rule<V, T>> {
    table
        .get(&TypeId::of::<V>())
        .into_iter()
        .flatten()
        .filter_map(|rule| rule.downcast_ref::<Rule<V, T>>())
}
