//! What a view is built into. Views above the leaves are generic over
//! it; each leaf is written once per backend.

use core::hash::Hash;

use bevy::prelude::*;

pub trait Backend: 'static {
    type World;
    type Node: Copy + Eq + Hash + Send + Sync + 'static;

    /// A new, empty node under `parent`, or at the root.
    fn spawn(
        world: &mut Self::World,
        parent: Option<Self::Node>,
    ) -> Self::Node;

    /// Hangs `child` under `parent`.
    fn adopt(
        world: &mut Self::World,
        parent: Self::Node,
        child: Self::Node,
    );
}

/// Bevy's ECS, with `bevy_ui` doing layout, text and picking.
pub struct Bevy;

impl Backend for Bevy {
    type World = World;
    type Node = Entity;

    fn spawn(world: &mut World, parent: Option<Entity>) -> Entity {
        let node = world.spawn(Node::default()).id();
        if let Some(parent) = parent {
            world.entity_mut(parent).add_child(node);
        }
        node
    }

    fn adopt(world: &mut World, parent: Entity, child: Entity) {
        world.entity_mut(parent).add_child(child);
    }
}
