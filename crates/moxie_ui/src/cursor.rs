//! Where the pointer is, for systems that poll it every frame.

use bevy::ecs::system::SystemParam;
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::UiScale;

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
