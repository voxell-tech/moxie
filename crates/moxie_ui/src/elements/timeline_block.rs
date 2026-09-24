use crate::reactive::FynixBuild;
use bevy::prelude::*;
use bevy_fynix::WorldEntityMut as _;
use fynix::element::element;

use super::patch::*;
use crate::drag::Dragged;

/// A block's header box: an absolutely positioned, bordered
/// container. Every `Node::Block` in a scene's animation tree gets
/// one of these - an action leaf has no children and stays a plain
/// `Frame` instead.
#[element(build = Self::build)]
pub struct TimelineBlock {
    #[elem(patch = PatchTop)]
    pub top: Val,
    #[elem(patch = PatchLeft)]
    pub left: Val,
    #[elem(patch = PatchWidth)]
    pub width: Val,
    #[elem(patch = PatchHeight)]
    pub height: Val,
    #[elem(default = theme.color.text.with_alpha(0.03), patch = PatchBackground, anim(
        duration = theme.motion.interact,
        ease = theme.motion.ease,
        on(Dragged, read = dragged_background),
    ))]
    pub background: Color,
    /// What `background` travels to while dragged.
    #[elem(ignore, default = theme.color.text.with_alpha(0.03))]
    pub dragged_background: Color,
    #[elem(default = theme.color.text.with_alpha(0.5), patch = PatchBorderColor, anim(
        duration = theme.motion.interact,
        ease = theme.motion.ease,
        on(Dragged, read = dragged_border),
    ))]
    pub border: Color,
    /// What `border` travels to while dragged.
    #[elem(ignore, default = theme.color.text.with_alpha(0.2))]
    pub dragged_border: Color,
    #[elem(patch = PatchSelected)]
    pub selected: bool,
}

impl TimelineBlock {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        build.insert((Node {
            position_type: PositionType::Absolute,
            // Without this the header row stretches to the whole
            // block's height instead of sitting at its top.
            align_items: AlignItems::Start,
            overflow: Overflow::clip(),
            ..default()
        },));
    }
}
