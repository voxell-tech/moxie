//! What views are built into.

use core::hash::Hash;

/// A world of nodes. Views above the elements are generic over it; each
/// element is written once per backend.
pub trait Backend: 'static {
    type World: 'static;
    type Node: Copy + Eq + Hash + Send + Sync + 'static;

    /// A new, empty node under `parent`, or at the root.
    fn spawn(
        world: &mut Self::World,
        parent: Option<Self::Node>,
    ) -> Self::Node;

    /// Removes `node` and every node under it.
    fn despawn(world: &mut Self::World, node: Self::Node);

    /// Sets the order of the children of `parent`, which `children`
    /// lists whole.
    fn reorder(
        world: &mut Self::World,
        parent: Self::Node,
        children: &[Self::Node],
    );

    /// A hook run when an element is mounted on `node`, for telling
    /// [`Mounted::unmount`](crate::Mounted::unmount) once it is gone.
    fn on_mount(_world: &mut Self::World, _node: Self::Node) {}
}
