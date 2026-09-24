use crate::reactive::FynixBuild;
use bevy::feathers::cursor::EntityCursor;
use bevy::prelude::*;
use bevy::ui_widgets::Button as ButtonBehavior;
use bevy::window::SystemCursorIcon;
use bevy_fynix::WorldEntityMut as _;
use bevy_fynix::tag::{Hovered, Pressed};
use fynix::element::element;

use super::Label;
use super::patch::*;
use crate::drag::Dragged;

/// One action's clip on the timeline: a colored, absolutely
/// positioned, bordered hit area, its name (if any) pinned to its
/// top-left corner - clipped rather than measured, so a bar too
/// narrow for it just shows nothing instead of overflowing its
/// neighbor.
#[element(build = Self::build)]
pub struct TimelineAction {
    /// Blank when the action has no name of its own.
    #[elem(child)]
    pub label: Label,
    #[elem(patch = PatchTop)]
    pub top: Val,
    #[elem(patch = PatchLeft)]
    pub left: Val,
    #[elem(patch = PatchWidth)]
    pub width: Val,
    #[elem(patch = PatchHeight)]
    pub height: Val,
    #[elem(default = theme.color.clip, patch = PatchBackground, anim(
        duration = theme.motion.interact,
        ease = theme.motion.ease,
        on(Dragged, read = dragged_fill),
        on(Pressed, read = press_fill),
        on(Hovered, read = hover_fill),
    ))]
    pub fill: Color,
    /// What `fill` travels to under the cursor.
    #[elem(ignore, default = theme.color.clip_hover)]
    pub hover_fill: Color,
    /// While held.
    #[elem(ignore, default = theme.color.clip_press)]
    pub press_fill: Color,
    /// While dragged.
    #[elem(ignore, default = theme.color.clip.with_alpha(0.2))]
    pub dragged_fill: Color,
    #[elem(default = Color::NONE, patch = PatchBorderColor, anim(
        duration = theme.motion.interact,
        ease = theme.motion.ease,
        on(Dragged, read = dragged_border),
    ))]
    pub border: Color,
    /// What `border` travels to while dragged.
    #[elem(ignore, default = Color::NONE)]
    pub dragged_border: Color,
    #[elem(patch = PatchSelected)]
    pub selected: bool,
}

impl TimelineAction {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        build.insert((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::new(
                    px(4),
                    Val::ZERO,
                    px(2),
                    Val::ZERO,
                ),
                overflow: Overflow::clip(),
                ..default()
            },
            ButtonBehavior,
            EntityCursor::System(SystemCursorIcon::Pointer),
        ));
    }
}
