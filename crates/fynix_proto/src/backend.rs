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

    /// Whether `node` is still alive.
    fn exists(world: &Self::World, node: Self::Node) -> bool;
}
