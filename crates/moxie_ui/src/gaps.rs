//! Generic pieces `bevy_fynix` lacks, kept free of moxie concepts.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use bevy_fynix::Signal;

/// Upstream: a signal that re-reads at every update and fires when
/// the value differs from the last one.
///
/// For state with no tick to watch. `read` runs twice per update, so
/// keep it cheap.
pub fn changing<T>(
    read: impl Fn(&World) -> T + Clone + Send + Sync + 'static,
) -> Signal<T>
where
    T: PartialEq + Send + Sync + 'static,
{
    let mut seen: Option<T> = None;
    let peek = read.clone();
    Signal::new(read, move |world: &World| {
        let current = peek(world);
        let fired = seen.as_ref() != Some(&current);
        seen = Some(current);
        fired
    })
}

/// Upstream: a signal of a projection of the resource `R`, firing
/// only when the projection differs, whatever else in `R` changed.
///
/// `R` has to be in the world whenever the signal is read.
pub fn projection<R: Resource, K>(
    project: impl Fn(&R) -> K + Clone + Send + Sync + 'static,
) -> Signal<K>
where
    K: PartialEq + Send + Sync + 'static,
{
    changing(move |world: &World| project(world.resource::<R>()))
}

/// Upstream: a signal of the current value of the state `S`, `None`
/// while the state is not initialised.
pub fn state_of<S: States>() -> Signal<Option<S>> {
    changing(|world: &World| {
        world.get_resource::<State<S>>().map(|s| s.get().clone())
    })
}

/// Upstream: a cursor icon that wins over every hovered node's own,
/// such as a grab held for the length of a drag.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct OverrideCursor(pub Option<SystemCursorIcon>);

/// Upstream: applies [`OverrideCursor`] to the primary window after
/// the hover cursor has been set.
pub struct OverrideCursorPlugin;

impl Plugin for OverrideCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverrideCursor>()
            .add_systems(Update, apply_override);
    }
}

fn apply_override(
    cursor: Res<OverrideCursor>,
    windows: Query<
        (Entity, Option<&CursorIcon>),
        With<PrimaryWindow>,
    >,
    mut commands: Commands,
) {
    let Some(icon) = cursor.0 else {
        return;
    };
    let wanted = CursorIcon::System(icon);
    for (window, current) in &windows {
        if current != Some(&wanted) {
            commands.entity(window).insert(wanted.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource)]
    struct Pair(u32, u32);

    #[test]
    fn changing_fires_on_first_read_and_on_difference() {
        let mut world = World::new();
        world.insert_resource(Pair(1, 1));
        let mut signal =
            changing(|world: &World| world.resource::<Pair>().0);

        assert!(signal.changed(&world));
        assert!(!signal.changed(&world));
        world.resource_mut::<Pair>().0 = 2;
        assert!(signal.changed(&world));
        assert_eq!(signal.get(&world), 2);
    }

    #[test]
    fn projection_ignores_other_fields() {
        let mut world = World::new();
        world.insert_resource(Pair(1, 1));
        let mut signal = projection::<Pair, _>(|pair| pair.0);

        assert!(signal.changed(&world));
        world.resource_mut::<Pair>().1 = 5;
        assert!(!signal.changed(&world));
        world.resource_mut::<Pair>().0 = 5;
        assert!(signal.changed(&world));
    }

    #[test]
    fn override_cursor_wins_on_the_window() {
        let mut app = App::new();
        app.add_plugins(OverrideCursorPlugin);
        let window = app.world_mut().spawn(PrimaryWindow).id();
        app.world_mut().resource_mut::<OverrideCursor>().0 =
            Some(SystemCursorIcon::Grabbing);
        app.update();

        assert_eq!(
            app.world().get::<CursorIcon>(window),
            Some(&CursorIcon::System(SystemCursorIcon::Grabbing))
        );
    }
}
