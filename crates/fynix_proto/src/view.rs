//! What a view is, and the kinds there are: leaves, composites built
//! out of other views, and wrappers around any view.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::backend::Backend;
use crate::cx::Cx;
use crate::transition::Tween;

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

/// A view that is one node of its own, with no views under it.
///
/// Its props are resolved against the rules in force, read into a
/// [`Snapshot`](Self::Snapshot) of plain values, and written onto the
/// node. A live leaf stays mounted, and is read and written again
/// whenever its snapshot changes.
pub trait Leaf<B: Backend, T>: Styled {
    /// Every prop's value at one moment, with the theme's defaults
    /// filled in.
    type Snapshot: Clone + PartialEq + Send + Sync + 'static;

    /// What the node needs besides what [`write`](Self::write) keeps
    /// up to date.
    fn prepare(world: &mut B::World, node: B::Node);

    fn snapshot(&self, world: &B::World, theme: &T)
    -> Self::Snapshot;

    fn write(
        snapshot: &Self::Snapshot,
        world: &mut B::World,
        node: B::Node,
    );

    /// Whether anything it holds can change after the build.
    fn is_live(&self) -> bool;

    /// This, with the rules in force applied.
    fn resolve(self, cx: &Cx<'_, B, T>) -> Self
    where
        T: 'static,
    {
        cx.resolve(self)
    }

    /// Edits a fresh snapshot of `node` before it is written, from
    /// what only the node knows.
    fn adjust(
        &self,
        _snapshot: &mut Self::Snapshot,
        _world: &B::World,
        _node: B::Node,
        _theme: &T,
    ) {
    }

    /// How the written values travel to a new snapshot. `None` snaps.
    fn tween(&self, _theme: &T) -> Option<Tween<Self::Snapshot>> {
        None
    }
}

impl<B: Backend, T: 'static, L: Leaf<B, T>> View<B, T> for L {
    fn build(self, cx: &mut Cx<'_, B, T>) -> B::Node {
        let leaf = L::resolve(self, cx);
        let node = cx.spawn();
        L::prepare(cx.world, node);
        let mut snapshot = leaf.snapshot(cx.world, cx.theme());
        leaf.adjust(&mut snapshot, cx.world, node, cx.theme());
        L::write(&snapshot, cx.world, node);
        if leaf.is_live() {
            cx.mounted().mount(node, leaf, snapshot);
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
view_seq!(V1);
view_seq!(V1, V2);
view_seq!(V1, V2, V3);
view_seq!(V1, V2, V3, V4);
view_seq!(V1, V2, V3, V4, V5);
view_seq!(V1, V2, V3, V4, V5, V6);
view_seq!(V1, V2, V3, V4, V5, V6, V7);
view_seq!(V1, V2, V3, V4, V5, V6, V7, V8);
view_seq!(V1, V2, V3, V4, V5, V6, V7, V8, V9);
view_seq!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10);
view_seq!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11);
view_seq!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12);

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

/// What any view can be turned into.
pub trait ViewExt<B: Backend, T>:
    View<B, T> + Sized + 'static
{
    /// This view, its type erased.
    fn boxed(self) -> AnyView<B, T> {
        AnyView::new(move |cx| self.build(cx))
    }
}

impl<B: Backend, T, V: View<B, T> + 'static> ViewExt<B, T> for V {}
