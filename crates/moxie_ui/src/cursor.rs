//! The pointer's position in logical screen space.

use core::fmt::Debug;

use bevy::ecs::system::SystemParam;
use bevy::picking::events::Pointer;
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::UiScale;

/// A picking event's pointer position in logical screen space.
pub trait PointerEventExt {
    fn logical(&self, scale: &UiScale) -> Vec2;
}

impl<E: Debug + Clone + Reflect> PointerEventExt for Pointer<E> {
    fn logical(&self, scale: &UiScale) -> Vec2 {
        self.pointer_location.position / scale.0
    }
}

/// The pointer in logical screen space.
#[derive(SystemParam)]
pub struct Cursor<'w, 's> {
    pointers: Query<'w, 's, &'static PointerLocation>,
    scale: Res<'w, UiScale>,
}

impl Cursor<'_, '_> {
    /// The pointer's position, if it has a location.
    pub fn position(&self) -> Option<Vec2> {
        self.pointers
            .iter()
            .find_map(|pointer| pointer.location())
            .map(|location| location.position / self.scale.0)
    }
}
