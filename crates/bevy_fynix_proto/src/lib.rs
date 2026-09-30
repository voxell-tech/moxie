//! The Bevy backend of `fynix_proto`, see `docs/fynix_rewrite.md`.
//!
//! The core owns views, set rules, props and transitions. This crate
//! says what a world and a node are in Bevy, writes the leaves and
//! composites against `bevy_ui`, and keeps mounted leaves in step with
//! the world every frame.

pub mod backend;
pub mod demo;
pub mod modifier;
pub mod mounted;
pub mod prop;
pub mod state;
pub mod tokens;
pub mod transition;
pub mod views;

#[cfg(test)]
mod tests;

use core::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
pub use fynix_proto::{
    AnyView, Cx, Leaf, Styled, View, ViewExt, ViewSeq,
};

pub use backend::Bevy;
pub use modifier::ModifierExt;
pub use mounted::Mounts;
pub use prop::{Prop, Signal, derived};
pub use state::{Hovered, Pressed, StateExt, Stateful};
pub use transition::{BevyMarker, ReducedMotion};

/// The theme views are built with, as a resource.
#[derive(Resource)]
pub struct Theme<T>(pub T);

/// Keeps every mounted view's bound props in step with the world.
/// The app inserts [`Theme<T>`] itself.
pub struct FynixProtoPlugin<T>(PhantomData<fn() -> T>);

impl<T> Default for FynixProtoPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Send + Sync + 'static> Plugin for FynixProtoPlugin<T> {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mounts<T>>()
            .init_resource::<ReducedMotion>()
            .add_systems(Update, mounted::update::<T>);
    }
}

/// Builds `view` at the root of the UI, with no rules in force.
pub fn mount<T: Send + Sync + 'static>(
    world: &mut World,
    view: impl View<Bevy, T>,
) -> Entity {
    world.resource_scope::<Mounts<T>, _>(|world, mut mounts| {
        world.resource_scope::<Theme<T>, _>(|world, theme| {
            let mut cx = Cx::new(world, &theme.0, &mut mounts.0);
            view.build(&mut cx)
        })
    })
}
