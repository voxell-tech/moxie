//! The core's props, bound to Bevy's world.

use bevy_ecs::world::World;

/// A prop of a view built into [`Bevy`](crate::Bevy).
pub type Prop<T> = fynix_proto::Prop<World, T>;

/// A value read from the world.
pub type Signal<T> = fynix_proto::Signal<World, T>;

/// A prop that follows whatever `read` returns. It fixes the world's
/// type, so a closure can use its argument at once.
pub fn derived<T>(
    read: impl Fn(&World) -> T + Send + Sync + 'static,
) -> Signal<T> {
    fynix_proto::derived(read)
}
