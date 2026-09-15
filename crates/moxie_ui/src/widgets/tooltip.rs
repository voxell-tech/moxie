//! A hover tag: text that appears near the cursor after a short
//! pause.
//!
//! Attached per-entity with [`tooltip`], the same way
//! `draggable_field` wires up a drag: the hover lives in one shared
//! `TooltipState` rather than on the element itself, so at most one
//! tag is ever showing.

use core::time::Duration;

use bevy::picking::events::{Out, Over, Pointer};
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy_fynix::{BevyFynix, WorldEntityMut};

use crate::theme::EditorTheme;

/// How long the pointer has to rest before the tag appears.
const DELAY: Duration = Duration::from_millis(500);

/// Where the tag sits relative to the cursor, clear of the pointer
/// hotspot.
const OFFSET: Vec2 = Vec2::new(12.0, 18.0);

/// What's hovered and how long it's been held, and the tag once the
/// delay has elapsed.
#[derive(Resource, Default)]
struct TooltipState {
    hovered: Option<(String, Duration)>,
    tag: Option<Entity>,
}

/// Registers [`tick`], the system [`tooltip`] otherwise has nothing
/// to drive it.
pub(crate) struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TooltipState>()
            .add_systems(Update, tick);
    }
}

/// Makes `elem` show `text` near the cursor after a short hover.
pub fn tooltip(
    elem: &mut impl WorldEntityMut,
    text: impl Into<String>,
) {
    let text = text.into();
    elem.observe(
        move |_: On<Pointer<Over>>,
              mut state: ResMut<TooltipState>| {
            state.hovered = Some((text.clone(), Duration::ZERO));
        },
    )
    .observe(
        |_: On<Pointer<Out>>,
         mut state: ResMut<TooltipState>,
         mut commands: Commands| {
            state.hovered = None;
            if let Some(tag) = state.tag.take() {
                commands.entity(tag).despawn();
            }
        },
    );
}

/// Counts the hover delay down, and spawns, moves, or despawns the
/// tag to match.
fn tick(
    time: Res<Time>,
    kernel: Res<BevyFynix<EditorTheme>>,
    scale: Res<UiScale>,
    pointers: Query<&PointerLocation>,
    mut state: ResMut<TooltipState>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    let Some(cursor) = pointers
        .iter()
        .find_map(|pointer| pointer.location())
        .map(|location| location.position / scale.0)
    else {
        return;
    };

    if state.hovered.is_none() {
        return;
    }

    if let Some(shown) = state.tag {
        if let Ok(mut node) = nodes.get_mut(shown) {
            node.left = px(cursor.x + OFFSET.x);
            node.top = px(cursor.y + OFFSET.y);
        }
        return;
    }

    let Some((text, elapsed)) = &mut state.hovered else {
        return;
    };
    *elapsed += time.delta();
    if *elapsed < DELAY {
        return;
    }
    let text = text.clone();

    let tag = tag(cursor, text, kernel.theme());
    state.tag = Some(commands.spawn(tag).id());
}

/// The tag itself: a small dark label, stacked with the same
/// [`GlobalZIndex`] convention as [`crate::drag::ghost`].
fn tag(
    cursor: Vec2,
    text: String,
    theme: &EditorTheme,
) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(cursor.x + OFFSET.x),
            top: px(cursor.y + OFFSET.y),
            padding: UiRect::axes(px(6), px(3)),
            border_radius: BorderRadius::all(px(theme.space.xs)),
            ..default()
        },
        BackgroundColor(theme.color.panel),
        GlobalZIndex(theme.layer.tooltip),
        Pickable::IGNORE,
        children![(
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(theme.text.small),
                ..default()
            },
            TextColor(theme.color.text),
            TextLayout::linebreak(LineBreak::NoWrap),
            Pickable::IGNORE,
        )],
    )
}
