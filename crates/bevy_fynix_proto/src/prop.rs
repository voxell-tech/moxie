//! The core's props, bound to Bevy's world.

use bevy_ecs::change_detection::Tick;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;

/// A prop of a view built into [`Bevy`](crate::Bevy).
pub type Prop<T> = fynix_proto::Prop<World, T>;

/// A value read from the world, with its change check.
pub type Signal<T> = fynix_proto::Signal<World, T>;

/// A view built again when its key changes, built into
/// [`Bevy`](crate::Bevy).
pub type Keyed<T, K> = fynix_proto::Keyed<crate::Bevy, T, K>;

/// One view per item, built into [`Bevy`](crate::Bevy).
pub type Each<T, I, K> = fynix_proto::Each<crate::Bevy, T, I, K>;

/// See [`fynix_proto::keyed`].
pub fn keyed<T, K>(
    key: Signal<K>,
    build: impl Fn(&K) -> fynix_proto::AnyView<crate::Bevy, T>
    + Send
    + Sync
    + 'static,
) -> Keyed<T, K>
where
    K: PartialEq + Clone + Send + Sync + 'static,
{
    fynix_proto::keyed(key, build)
}

/// See [`fynix_proto::each`].
pub fn each<T, I, K>(
    items: Signal<Vec<I>>,
    key: fn(&I) -> K,
    build: impl Fn(&I) -> fynix_proto::AnyView<crate::Bevy, T>
    + Send
    + Sync
    + 'static,
) -> Each<T, I, K>
where
    K: PartialEq + Clone + Send + Sync + 'static,
{
    fynix_proto::each(items, key, build)
}

/// A read of the world still waiting for its change check.
pub type Derived<T> = fynix_proto::Derived<World, T>;

/// A read of whatever `read` returns, to be given its change check
/// with [`when`](fynix_proto::Derived::when). It fixes the world's
/// type, so a closure can use its argument at once.
pub fn derived<T>(
    read: impl Fn(&World) -> T + Send + Sync + 'static,
) -> Derived<T> {
    fynix_proto::derived(read)
}

/// A check that fires at every update, for a read with no source to
/// watch.
pub fn every_frame()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    |_| true
}

/// The change tick a check saw last.
#[derive(Default)]
struct Seen {
    tick: Option<Tick>,
    /// Whether a write could still land on `tick`.
    open: bool,
}

impl Seen {
    /// Whether `tick` differs from the one seen before, and takes it
    /// as seen. `None` is an absent source.
    fn fires(&mut self, world: &World, tick: Option<Tick>) -> bool {
        let fires = self.open || self.tick != tick;
        self.tick = tick;
        // A write made without a system running in between gets the
        // same tick as the one seen, and would go unnoticed.
        self.open = tick == Some(world.read_change_tick());
        fires
    }
}

/// A signal reading the resource `R`, re-read when `R` changes.
/// `R` has to be in the world whenever the signal is read.
pub fn resource<R: Resource, T>(
    read: impl Fn(&R) -> T + Send + Sync + 'static,
) -> Signal<T> {
    let mut seen = Seen::default();
    Signal::new(
        move |world: &World| read(world.resource::<R>()),
        move |world: &World| {
            let tick = world
                .get_resource_change_ticks::<R>()
                .map(|ticks| ticks.changed);
            seen.fires(world, tick)
        },
    )
}

/// A signal reading the component `C` of `entity`, re-read when `C`
/// changes, or is added or removed. `read` gets `None` while `C` is
/// absent.
pub fn component<C: Component, T>(
    entity: Entity,
    read: impl Fn(Option<&C>) -> T + Send + Sync + 'static,
) -> Signal<T> {
    let mut seen = Seen::default();
    Signal::new(
        move |world: &World| read(world.get::<C>(entity)),
        move |world: &World| {
            let tick = world
                .get_entity(entity)
                .ok()
                .and_then(|entity| entity.get_change_ticks::<C>())
                .map(|ticks| ticks.changed);
            seen.fires(world, tick)
        },
    )
}
