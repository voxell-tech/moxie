use crate::reactive::FynixBuild;
use bevy::prelude::*;
use bevy_fynix::WorldEntityMut as _;
use fynix::element::element;

use super::patch::*;

/// A small square turned 45 degrees - the keyframe marker beside an
/// animatable field's name.
#[element(build = Self::build)]
pub struct Diamond {
    #[elem(default = ::NONE, patch = PatchBackground)]
    pub background: Color,
}

impl Diamond {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        build.insert((
            Node {
                width: px(6),
                height: px(6),
                margin: UiRect::horizontal(px(2)),
                ..default()
            },
            UiTransform::from_rotation(Rot2::degrees(45.0)),
        ));
    }
}
