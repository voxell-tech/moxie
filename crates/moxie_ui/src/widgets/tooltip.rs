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
use bevy_fynix::WorldEntityMut;
use fynix::prelude::*;

use crate::elements::{Label, MenuSurface};
use crate::reactive::{BevyUi, watch_root};
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

/// The tag's own surface, so [`tick`] can reposition it without
/// rebuilding: [`watch_root`] hands back nothing to hold onto.
#[derive(Component)]
struct TooltipMark;

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
    scale: Res<UiScale>,
    pointers: Query<&PointerLocation>,
    mut state: ResMut<TooltipState>,
    mut nodes: Query<&mut Node, With<TooltipMark>>,
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

    if state.tag.is_some() {
        // Only ever one shown at a time, so nothing to key this on.
        for mut node in &mut nodes {
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

    // `Pickable::IGNORE`, or this full-screen node blocks the very
    // hover that showed it: it swallows the pointer, the source loses
    // `Over`, the tag despawns, hover resumes, and it loops.
    let root = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    commands.queue(move |world: &mut World| {
        spawn_tag(world, root, cursor, text)
    });
    state.tag = Some(root);
}

/// The tag itself, on [`MenuSurface`] - the same popup surface a
/// context menu or a dropdown's own list uses - stacked above both
/// per [`crate::theme::Layers::tooltip`].
fn spawn_tag(
    world: &mut World,
    root: Entity,
    cursor: Vec2,
    text: String,
) {
    watch_root::<EditorTheme>(world, root, move |ui: &mut BevyUi| {
        let z = ui.theme.layer.tooltip;
        let text_color = ui.theme.color.text;
        let size = ui.theme.text.small;

        ui.elem(elem!(
            !MenuSurface,
            z = Some(z),
            inset = UiRect::new(
                px(cursor.x + OFFSET.x),
                auto(),
                px(cursor.y + OFFSET.y),
                auto()
            ),
        ))
        .insert((TooltipMark, Pickable::IGNORE))
        .with({
            let text = text.clone();
            move |ui| {
                ui.elem(elem!(
                    Label,
                    text = text.clone(),
                    size = size,
                    color = text_color,
                    wrap = false
                ))
                .insert(Pickable::IGNORE);
            }
        });
    });
}
