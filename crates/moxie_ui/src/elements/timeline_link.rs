use crate::reactive::FynixBuild;
use bevy::prelude::*;
use bevy_fynix::WorldEntityMut as _;
use fynix::element::element;

use super::patch::*;

/// A right-angle line joining one flow child's start to the next:
/// down its left edge, then along its bottom. Ignores the pointer.
#[element(build = Self::build)]
pub struct TimelineLink {
    #[elem(patch = PatchTop)]
    pub top: Val,
    #[elem(patch = PatchLeft)]
    pub left: Val,
    #[elem(patch = PatchWidth)]
    pub width: Val,
    #[elem(patch = PatchHeight)]
    pub height: Val,
    #[elem(default = theme.color.text_dim, patch = PatchBorderColor)]
    pub color: Color,
}

impl TimelineLink {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        build.insert((
            Node {
                position_type: PositionType::Absolute,
                border: UiRect {
                    left: px(3),
                    bottom: px(3),
                    ..default()
                },
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
}
