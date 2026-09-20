use crate::reactive::FynixBuild;
use bevy::feathers::cursor::EntityCursor;
use bevy::prelude::*;
use bevy::ui_widgets::Button as ButtonBehavior;
use bevy::window::SystemCursorIcon;
use bevy_fynix::WorldEntityMut as _;
use bevy_fynix::tag::{Hovered, Pressed};
use fynix::element::element;

use super::patch::*;
use super::{Icon, Label};
use crate::drag::Dragged;

/// An action icon's full size, in logical pixels.
pub const ACTION_ICON_SIZE: f32 = 14.0;
/// Gap between an action icon and its label.
const ACTION_ICON_GAP: f32 = 4.0;
/// The bar width from which the icon is at full size.
const ICON_FULL_AT: f32 = 28.0;
/// The bar width up to which the icon is gone.
const ICON_GONE_AT: f32 = 6.0;

/// One action's clip on the timeline: a colored, absolutely
/// positioned, bordered hit area, its icon (if any) and name (if any)
/// centered vertically at its left edge - clipped rather than
/// measured, so a bar too narrow for them just shows nothing instead
/// of overflowing its neighbor. The icon shrinks and fades as the bar
/// narrows ([`fit_action_icons`]).
#[element(build = Self::build)]
pub struct TimelineAction {
    /// Sized at [`ACTION_ICON_SIZE`]: [`fit_action_icons`] takes it
    /// over from there.
    #[elem(child)]
    pub icon: Option<Icon>,
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
    /// What `fill` travels to while held.
    #[elem(ignore, default = theme.color.clip_press)]
    pub press_fill: Color,
    /// What `fill` travels to while dragged.
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
                padding: UiRect::left(px(4)),
                column_gap: px(ACTION_ICON_GAP),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            ButtonBehavior,
            EntityCursor::System(SystemCursorIcon::Pointer),
            ActionClip,
        ));
    }
}

/// A [`TimelineAction`]'s node, for [`fit_action_icons`] to find.
#[derive(Component)]
pub struct ActionClip;

/// How much of its full size an action icon keeps on a bar `width`
/// logical pixels wide: all of it on a wide bar, none on one too
/// narrow to show it.
///
/// A new clip gets this from its placement as it is built, so its
/// icon never shows at the wrong size while waiting on a layout.
pub fn icon_fit(width: f32) -> f32 {
    ((width - ICON_GONE_AT) / (ICON_FULL_AT - ICON_GONE_AT))
        .clamp(0.0, 1.0)
}

/// Scales every action's icon down and fades it out as its bar
/// narrows ([`icon_fit`]).
///
/// Reads the laid-out width rather than the placement, so a bar
/// being dragged narrower follows along. A clip not laid out yet has
/// no width to read: it keeps what it was built with.
pub fn fit_action_icons(
    clips: Query<(&ComputedNode, &Children), With<ActionClip>>,
    mut icons: Query<(&mut Node, &mut ImageNode)>,
) {
    for (computed, children) in &clips {
        if computed.size() == Vec2::ZERO {
            continue;
        }
        let fit = icon_fit(
            computed.size().x * computed.inverse_scale_factor(),
        );

        for child in children {
            let Ok((mut node, mut image)) = icons.get_mut(*child)
            else {
                continue;
            };

            let size = px(ACTION_ICON_SIZE * fit);
            if node.width != size {
                node.width = size;
                node.height = size;
            }
            if image.color.alpha() != fit {
                image.color.set_alpha(fit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip laid out `width` wide, holding one icon; returns the
    /// icon after [`fit_action_icons`] has run.
    fn fitted(width: f32) -> (Node, f32) {
        let mut world = World::new();
        let icon =
            world.spawn((Node::default(), ImageNode::default())).id();
        world
            .spawn((
                ActionClip,
                ComputedNode {
                    size: Vec2::new(width, 26.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
            ))
            .add_child(icon);

        world
            .run_system_cached(fit_action_icons)
            .expect("system runs");

        let node = world.get::<Node>(icon).unwrap().clone();
        let alpha =
            world.get::<ImageNode>(icon).unwrap().color.alpha();
        (node, alpha)
    }

    #[test]
    fn wide_bar_shows_the_icon_in_full() {
        let (node, alpha) = fitted(200.0);
        assert_eq!(node.width, px(ACTION_ICON_SIZE));
        assert_eq!(node.height, px(ACTION_ICON_SIZE));
        assert_eq!(alpha, 1.0);
    }

    #[test]
    fn narrowing_bar_scales_and_fades_together() {
        let (node, alpha) =
            fitted((ICON_FULL_AT + ICON_GONE_AT) / 2.0);
        assert_eq!(node.width, px(ACTION_ICON_SIZE / 2.0));
        assert_eq!(alpha, 0.5);
    }

    #[test]
    fn bar_too_narrow_for_it_leaves_nothing() {
        let (node, alpha) = fitted(2.0);
        assert_eq!(node.width, px(0.0));
        assert_eq!(alpha, 0.0);
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn unlaid_out_clip_keeps_its_built_size() {
        let mut world = World::new();
        let icon = world
            .spawn((
                Node {
                    width: px(ACTION_ICON_SIZE),
                    height: px(ACTION_ICON_SIZE),
                    ..default()
                },
                ImageNode::default(),
            ))
            .id();
        world
            .spawn((ActionClip, ComputedNode::default()))
            .add_child(icon);

        world
            .run_system_cached(fit_action_icons)
            .expect("system runs");

        let node = world.get::<Node>(icon).unwrap();
        assert_eq!(node.width, px(ACTION_ICON_SIZE));
        assert_eq!(
            world.get::<ImageNode>(icon).unwrap().color.alpha(),
            1.0
        );
    }

    #[test]
    fn fit_is_a_ramp_between_the_two_widths() {
        assert_eq!(icon_fit(ICON_GONE_AT), 0.0);
        assert_eq!(icon_fit(ICON_FULL_AT), 1.0);
        assert_eq!(icon_fit(0.0), 0.0);
        assert_eq!(icon_fit(1000.0), 1.0);
    }
}
