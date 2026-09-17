//! A hover tag: text that appears near the cursor after a short
//! pause.
//!
//! Attached per-entity with [`tooltip`], the same way
//! `draggable_field` wires up a drag: the hover lives in one shared
//! `TooltipState` rather than on the element itself, so at most one
//! tag is ever showing.
//!
//! The tag is itself interactable, not just decoration: it stays up
//! while the pointer sits on it, not only on the source, so a link or
//! a button inside one can be reached and clicked.

use core::time::Duration;

use bevy::picking::events::{Out, Over, Pointer};
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy_fynix::WorldEntityMut;
use fynix::prelude::*;

use crate::elements::{Frame, Label, MenuSurface};
use crate::reactive::{BevyUi, watch_root};
use crate::theme::EditorTheme;

/// How long the pointer has to rest on the source before the tag
/// appears.
const DELAY: Duration = Duration::from_millis(500);

/// How long neither the source nor the tag may be hovered before it
/// hides - long enough to cross the gap between them to reach the
/// tag itself.
const HIDE_GRACE: Duration = Duration::from_millis(150);

/// Where the tag sits relative to the cursor it appeared at, clear of
/// the pointer hotspot. Fixed once shown: the tag no longer tracks
/// the pointer, since a pointer reaching into it to click something
/// would otherwise be chasing a moving target.
const OFFSET: Vec2 = Vec2::new(12.0, 18.0);

/// What's on its way to showing, what's shown, and whether the
/// pointer is still on the source or the tag itself - it hides only
/// once neither is true, past [`HIDE_GRACE`].
#[derive(Resource, Default)]
struct TooltipState {
    pending: Option<(String, Duration)>,
    shown: Option<Entity>,
    hovered_source: bool,
    hovered_tag: bool,
    /// Counts up once neither is hovered; `None` while either is.
    hiding: Option<Duration>,
}

/// The tag's own surface, so its `Over`/`Out` can be told from the
/// source's.
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

/// Makes `elem` show `text` near the cursor after a short hover, and
/// keeps it up as long as the pointer stays on `elem` or the tag.
pub fn tooltip(
    elem: &mut impl WorldEntityMut,
    text: impl Into<String>,
) {
    let text = text.into();
    elem.observe(
        move |_: On<Pointer<Over>>,
              mut state: ResMut<TooltipState>| {
            state.hovered_source = true;
            state.hiding = None;
            if state.shown.is_none() {
                state.pending.get_or_insert_with(|| {
                    (text.clone(), Duration::ZERO)
                });
            }
        },
    )
    .observe(
        |_: On<Pointer<Out>>, mut state: ResMut<TooltipState>| {
            state.hovered_source = false;
            if state.shown.is_none() {
                state.pending = None;
            }
        },
    );
}

/// Counts the show delay down, spawns the tag once it elapses, and
/// counts the hide grace once neither the source nor the tag is
/// hovered any longer.
fn tick(
    time: Res<Time>,
    scale: Res<UiScale>,
    pointers: Query<&PointerLocation>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    if let Some(shown) = state.shown {
        if state.hovered_source || state.hovered_tag {
            state.hiding = None;
            return;
        }
        let hiding = state.hiding.get_or_insert(Duration::ZERO);
        *hiding += time.delta();
        if *hiding >= HIDE_GRACE {
            commands.entity(shown).despawn();
            state.shown = None;
            state.pending = None;
            state.hiding = None;
        }
        return;
    }

    if !state.hovered_source {
        state.pending = None;
        return;
    }

    let Some((text, elapsed)) = &mut state.pending else {
        return;
    };
    *elapsed += time.delta();
    if *elapsed < DELAY {
        return;
    }
    let text = text.clone();

    let Some(cursor) = pointers
        .iter()
        .find_map(|pointer| pointer.location())
        .map(|location| location.position / scale.0)
    else {
        return;
    };

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
    state.shown = Some(root);
}

/// The tag itself, on [`MenuSurface`] - the same popup surface a
/// context menu or a dropdown's own list uses - stacked above both
/// per [`crate::theme::Layers::tooltip`]. Pickable, not
/// `Pickable::IGNORE`, so its own `Over`/`Out` can hold it open and
/// content inside it (a link, say) can be clicked.
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
        .insert(TooltipMark)
        .observe(
            |_: On<Pointer<Over>>,
             mut state: ResMut<TooltipState>| {
                state.hovered_tag = true;
                state.hiding = None;
            },
        )
        .observe(
            |_: On<Pointer<Out>>, mut state: ResMut<TooltipState>| {
                state.hovered_tag = false;
            },
        )
        .with({
            let text = text.clone();
            move |ui| {
                let h = ui.theme.space.md;
                let v = ui.theme.space.xs;
                // Its own room beyond `MenuSurface`'s own padding:
                // that's sized for a menu row, snug on a bare tag.
                ui.elem(elem!(
                    Frame,
                    padding = UiRect::axes(px(h), px(v))
                ))
                .with(move |ui| {
                    ui.elem(elem!(
                        Label,
                        text = text.clone(),
                        size = size,
                        color = text_color,
                        wrap = false
                    ));
                });
            }
        });
    });
}
