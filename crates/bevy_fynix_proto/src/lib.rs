//! A throwaway prototype of the fynix rewrite in `docs/fynix_rewrite.md`.
//!
//! Views are structs that own their own props and hold other views
//! whole. Set rules restyle every view of a kind within a scope, a
//! call-site value beats any rule, and whatever is left unset falls
//! back to the theme's tokens. Bound props are re-read every frame and
//! written only when they change.

pub mod backend;
pub mod cx;
pub mod demo;
pub mod modifier;
pub mod mounted;
pub mod prop;
pub mod state;
pub mod tokens;
pub mod transition;
pub mod view;
pub mod views;

#[cfg(test)]
mod tests;

use core::marker::PhantomData;

use bevy::prelude::*;

pub use backend::{Backend, Bevy};
pub use cx::Cx;
pub use modifier::ModifierExt;
pub use prop::{Prop, Signal, derived};
pub use state::{Hovered, Pressed, StateExt, Stateful};
pub use transition::{Interpolate, ReducedMotion};
pub use view::{AnyView, Leaf, Styled, View, ViewExt, ViewSeq};

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
        app.init_resource::<mounted::Mounted<T>>()
            .init_resource::<ReducedMotion>()
            .add_systems(Update, mounted::update::<T>);
    }
}

/// Builds `view` at the root of the UI, with no rules in force.
pub fn mount<T: Send + Sync + 'static>(
    world: &mut World,
    view: impl View<Bevy, T>,
) -> Entity {
    world.resource_scope::<Theme<T>, _>(|world, theme| {
        let mut cx = Cx::new(world, &theme.0);
        view.build(&mut cx)
    })
}
