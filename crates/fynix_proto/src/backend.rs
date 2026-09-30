//! What views are built into.

use core::hash::Hash;

/// A world of nodes. Views above the leaves are generic over it; each
/// leaf is written once per backend.
pub trait Backend: 'static {
    type World: 'static;
    type Node: Copy + Eq + Hash + Send + Sync + 'static;

    /// A new, empty node under `parent`, or at the root.
    fn spawn(
        world: &mut Self::World,
        parent: Option<Self::Node>,
    ) -> Self::Node;

    /// A hook run when a leaf is mounted on `node`, for telling
    /// [`Mounted::unmount`](crate::Mounted::unmount) once it is gone.
    fn on_mount(_world: &mut Self::World, _node: Self::Node) {}
}
