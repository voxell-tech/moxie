//! What a view is, and the three kinds there are: leaves, composites
//! built out of other views, and modifiers wrapping any view.

use bevy::prelude::*;

use crate::backend::{Backend, Bevy};
use crate::cx::Cx;
use crate::mounted::Mounted;

/// Something that can be built under a node, with the theme `T`.
pub trait View<B: Backend, T> {
    fn build(self, cx: &mut Cx<'_, B, T>) -> B::Node;
}

/// A view set rules can restyle: every prop can be left unset, and a
/// value left unset can be filled from below.
pub trait Styled: Sized + Send + Sync + 'static {
    /// Every prop unset.
    fn unset() -> Self;

    /// This, with each prop it left unset taken from `below`.
    fn over(self, below: Self) -> Self;
}

/// A view that is one Bevy node of its own, with no views under it.
///
/// Its props are resolved against the rules in force, then read into a
/// [`Snapshot`](Self::Snapshot) of plain values and written onto the
/// node. A leaf holding a bound prop stays mounted, and is read and
/// written again whenever its snapshot changes.
pub trait Leaf<T>: Styled {
    /// Every prop's value at one moment, with the theme's defaults
    /// filled in.
    type Snapshot: Clone + PartialEq + Send + Sync + 'static;

    /// What the node needs besides what [`write`](Self::write) keeps
    /// up to date.
    fn prepare(world: &mut World, node: Entity);

    fn snapshot(&self, world: &World, theme: &T) -> Self::Snapshot;

    fn write(
        snapshot: &Self::Snapshot,
        world: &mut World,
        node: Entity,
    );

    /// Whether anything it holds can change after the build.
    fn is_live(&self) -> bool;
}

impl<T: Send + Sync + 'static, L: Leaf<T>> View<Bevy, T> for L {
    fn build(self, cx: &mut Cx<'_, Bevy, T>) -> Entity {
        let leaf = cx.resolve(self);
        let node = cx.spawn();
        L::prepare(cx.world, node);
        let snapshot = leaf.snapshot(cx.world, cx.theme());
        L::write(&snapshot, cx.world, node);
        if leaf.is_live() {
            cx.world
                .resource_mut::<Mounted<T>>()
                .mount(node, leaf, snapshot);
        }
        node
    }
}

/// Views built one after another under the same node: a tuple of
/// views, or a `Vec` of one kind.
pub trait ViewSeq<B: Backend, T> {
    fn build_each(self, cx: &mut Cx<'_, B, T>) -> Vec<B::Node>;
}

impl<B: Backend, T, V: View<B, T>> ViewSeq<B, T> for Vec<V> {
    fn build_each(self, cx: &mut Cx<'_, B, T>) -> Vec<B::Node> {
        self.into_iter().map(|view| view.build(cx)).collect()
    }
}

macro_rules! view_seq {
    ($($view:ident),*) => {
        impl<B: Backend, T, $($view: View<B, T>),*> ViewSeq<B, T>
            for ($($view,)*)
        {
            #[allow(non_snake_case, unused_variables)]
            fn build_each(self, cx: &mut Cx<'_, B, T>) -> Vec<B::Node> {
                let ($($view,)*) = self;
                vec![$($view.build(cx)),*]
            }
        }
    };
}

view_seq!();
view_seq!(A);
view_seq!(A, B1);
view_seq!(A, B1, C);
view_seq!(A, B1, C, D);
view_seq!(A, B1, C, D, E);
view_seq!(A, B1, C, D, E, F);

/// A view's build, its type erased.
type BuildFn<B, T> =
    dyn for<'a> FnOnce(&mut Cx<'a, B, T>) -> <B as Backend>::Node;

/// Any view, its type erased: for storing views of different kinds
/// together, or keeping a deep view's type from growing.
pub struct AnyView<B: Backend, T>(Box<BuildFn<B, T>>);

impl<B: Backend, T> AnyView<B, T> {
    /// A view built by `build`, for one written in place.
    pub fn new(
        build: impl for<'a> FnOnce(&mut Cx<'a, B, T>) -> B::Node + 'static,
    ) -> Self {
        Self(Box::new(build))
    }
}

impl<B: Backend, T> View<B, T> for AnyView<B, T> {
    fn build(self, cx: &mut Cx<'_, B, T>) -> B::Node {
        (self.0)(cx)
    }
}

/// What any view can be modified with.
pub trait ViewExt<B: Backend, T>:
    View<B, T> + Sized + 'static
{
    /// This view, its type erased.
    fn boxed(self) -> AnyView<B, T> {
        AnyView::new(move |cx| self.build(cx))
    }
}

impl<B: Backend, T, V: View<B, T> + 'static> ViewExt<B, T> for V {}
