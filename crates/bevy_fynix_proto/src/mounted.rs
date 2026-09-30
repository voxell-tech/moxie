//! The core's mounted elements, kept as a resource and updated each
//! frame.

use core::ops::{Deref, DerefMut};

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use bevy_time::Time;

use crate::backend::Unmounted;
use crate::state::DirtyNodes;
use crate::transition::ReducedMotion;
use crate::{Bevy, Theme};

/// Every mounted element built with the theme `T`.
#[derive(Resource)]
pub struct Mounts<T: 'static>(pub fynix_proto::Mounted<Bevy, T>);

impl<T: 'static> Default for Mounts<T> {
    fn default() -> Self {
        Self(fynix_proto::Mounted::default())
    }
}

impl<T: 'static> Deref for Mounts<T> {
    type Target = fynix_proto::Mounted<Bevy, T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: 'static> DerefMut for Mounts<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Brings every mounted element up to date.
pub(crate) fn update<T: Send + Sync + 'static>(world: &mut World) {
    let tick = fynix_proto::Tick {
        delta: world.resource::<Time>().delta(),
        reduced_motion: world
            .get_resource::<ReducedMotion>()
            .is_some_and(|reduced| reduced.0),
    };
    let gone =
        core::mem::take(&mut world.resource_mut::<Unmounted>().0);
    let dirty =
        core::mem::take(&mut world.resource_mut::<DirtyNodes>().0);
    world.resource_scope::<Mounts<T>, _>(|world, mut mounts| {
        for node in gone {
            mounts.unmount(node);
        }
        for node in dirty {
            mounts.mark_dirty(node);
        }
        world.resource_scope::<Theme<T>, _>(|world, theme| {
            mounts.0.update(world, &theme.0, tick);
        });
    });
}
