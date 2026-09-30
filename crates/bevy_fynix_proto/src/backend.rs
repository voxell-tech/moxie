//! The Bevy backend of the core.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_ui::Node;

/// Bevy's ECS, with `bevy_ui` doing layout, text and picking.
pub struct Bevy;

impl fynix_proto::Backend for Bevy {
    type World = World;
    type Node = Entity;

    fn spawn(world: &mut World, parent: Option<Entity>) -> Entity {
        let node = world.spawn(Node::default()).id();
        if let Some(parent) = parent {
            world.entity_mut(parent).add_child(node);
        }
        node
    }

    fn exists(world: &World, node: Entity) -> bool {
        world.get_entity(node).is_ok()
    }
}
