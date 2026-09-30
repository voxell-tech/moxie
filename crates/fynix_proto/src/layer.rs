//! State rules as they reach one element: props put on top of it while
//! a state holds, and taken off when it stops.

use alloc::vec::Vec;

use crate::backend::Backend;
use crate::rules::When;
use crate::transition::Tween;
use crate::view::Element;

/// The props a state rule sets on an element, and the state they wait
/// on.
pub(crate) struct Layer<B: Backend, E> {
    /// Built by the rule from an unset view, so only the props it
    /// touches are set.
    pub view: E,
    pub when: When<B>,
    /// The node the state is read on. `None` is the element's own.
    pub on: Option<B::Node>,
    /// Whether the rule was written on the element itself, so it beats
    /// the call site. One written on an ancestor only fills what the
    /// call site left unset.
    pub own: bool,
}

impl<B: Backend, E> Layer<B, E> {
    /// The node the state is read on, for an element on `node`.
    pub fn on(&self, node: B::Node) -> B::Node {
        self.on.unwrap_or(node)
    }
}

/// What a mounted element keeps beside its props.
pub(crate) struct Live<B: Backend, E, S> {
    /// Weakest first.
    pub layers: Vec<Layer<B, E>>,
    /// The props the call site set.
    pub call: u64,
    pub tween: Option<Tween<S>>,
}

impl<B: Backend, E, S> Live<B, E, S> {
    /// The snapshot of `element` with every layer whose state holds
    /// put on top, each layer taken off again after.
    pub fn snapshot<T>(
        &mut self,
        element: &mut E,
        world: &B::World,
        node: B::Node,
        theme: &T,
    ) -> S
    where
        E: Element<B, T, Snapshot = S>,
    {
        let mut swapped = Vec::new();
        for (index, layer) in self.layers.iter_mut().enumerate() {
            if !(layer.when.holds)(world, layer.on(node)) {
                continue;
            }
            let allowed =
                if layer.own { u64::MAX } else { !self.call };
            let mask = layer.view.set_mask() & allowed;
            element.swap_props(&mut layer.view, mask);
            swapped.push((index, mask));
        }
        let mut snapshot = element.snapshot(world, theme);
        element.adjust(&mut snapshot, world, node, theme);
        for (index, mask) in swapped.into_iter().rev() {
            element.swap_props(&mut self.layers[index].view, mask);
        }
        snapshot
    }

    /// Whether a bound prop in any layer may have changed. Every
    /// check runs, as each keeps its own memory.
    pub fn changed<T>(&mut self, world: &B::World) -> bool
    where
        E: Element<B, T, Snapshot = S>,
    {
        self.layers.iter_mut().fold(false, |changed, layer| {
            layer.view.changed(world) | changed
        })
    }
}
