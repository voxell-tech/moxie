//! The Bevy backend of the core.

use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::Despawn;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use bevy_ecs::world::World;
use bevy_ui::Node;

/// Bevy's ECS, with `bevy_ui` doing layout, text and picking.
pub struct Bevy;

/// The UI nodes despawned since the last update.
#[derive(Resource, Default, Debug)]
pub struct Unmounted(pub Vec<Entity>);

/// Queues every despawned [`Node`]. Only a queue, as the mounts are
/// out of the world while they update, and a despawn can happen then.
pub(crate) fn queue_unmounted(
    despawn: On<Despawn, Node>,
    mut queue: ResMut<Unmounted>,
) {
    queue.0.push(despawn.entity);
}

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

    fn despawn(world: &mut World, node: Entity) {
        if let Ok(node) = world.get_entity_mut(node) {
            node.despawn();
        }
    }

    fn reorder(world: &mut World, parent: Entity, children: &[Entity]) {
        world.entity_mut(parent).replace_children(children);
    }
}
