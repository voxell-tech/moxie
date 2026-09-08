//! Shared drag chrome: the tag that follows the cursor while
//! something is being dragged, and the grab cursor held for the
//! drag's duration.

use bevy::feathers::cursor::{EntityCursor, OverrideCursor};
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;

use crate::theme::EditorTheme;

/// Where the ghost sits relative to the cursor, so the pointer lands
/// just inside it rather than on its corner.
pub const GHOST_OFFSET: Vec2 = Vec2::new(-8.0, -9.0);

/// A labelled tag that follows the cursor while something is dragged.
///
/// [`Pickable::IGNORE`] because it sits directly under the cursor:
/// seen by the pointer it would be the only thing ever dragged over,
/// and no drop target would light up. Reposition it on each
/// `Pointer<Drag>` with [`follow`].
pub fn ghost(
    cursor: Vec2,
    label: String,
    theme: &EditorTheme,
) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(cursor.x + GHOST_OFFSET.x),
            top: px(cursor.y + GHOST_OFFSET.y),
            padding: UiRect::axes(px(6), px(2)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(theme.color.accent.with_alpha(0.85)),
        GlobalZIndex(theme.layer.drag),
        Pickable::IGNORE,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(theme.palette.base[0]),
            TextLayout::linebreak(LineBreak::NoWrap),
            Pickable::IGNORE,
        )],
    )
}

/// Moves a [`ghost`]'s node to `cursor`, in logical screen space.
pub fn follow(node: &mut Node, cursor: Vec2) {
    node.left = px(cursor.x + GHOST_OFFSET.x);
    node.top = px(cursor.y + GHOST_OFFSET.y);
}

const GRABBING: EntityCursor =
    EntityCursor::System(SystemCursorIcon::Grabbing);

/// Shows the grabbing cursor for the drag, over whatever the pointer
/// crosses. A no-op if something else already overrides the cursor.
pub fn grab(cursor: &mut OverrideCursor) {
    if cursor.0.is_none() {
        cursor.0 = Some(GRABBING);
    }
}

/// Drops the grabbing cursor, but only if [`grab`] is what set it.
pub fn ungrab(cursor: &mut OverrideCursor) {
    if cursor.0 == Some(GRABBING) {
        cursor.0 = None;
    }
}
