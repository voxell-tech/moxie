//! Retiming a node by dragging one of its box's edges: the left edge
//! edits `delay`, the right edge `duration` (leaves only - a block has
//! no `duration`). Dedicated handles leave the body free for
//! `reorder`'s merge gesture.
//!
//! Nothing writes [`EditorScene`] until [`DragEnd`]: the box list
//! watches it, so a mid-drag write would rebuild the dragged box out
//! from under the gesture. Each `Pointer<Drag>` instead lays out a
//! scratch copy of the tree with the tentative edit and pushes the
//! result onto the spawned entities by path. Escape re-lays the
//! untouched tree to undo the preview.

use core::time::Duration;

use bevy::feathers::cursor::EntityCursor;
use bevy::input::ButtonInput;
use bevy::picking::events::{
    Click, Drag, DragEnd, DragStart, Pointer, Press, Release,
};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::SystemCursorIcon;
use bevy_fynix::WorldEntityMut;
use bevy_motiongfx::scene::backend::Backend;
use fynix::prelude::*;
use motiongfx_scene::block::Node as SceneNode;
use moxie_ui::reactive::FynixHost;

use super::super::action::{node_at, node_at_mut};
use super::block_layout::{self, Placed};
use super::{BlockFoldState, RebuildTick};
use crate::{EditorScene, EditorSettings, TimelineView};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dragging>()
        .add_systems(Update, cancel_on_escape);
}

/// An edge handle's width.
pub(crate) const EDGE_HANDLE_PX: f32 = 6.0;

/// The field an edge handle edits.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    /// The left edge: `delay`.
    Delay,
    /// The right edge: `duration`.
    Resize,
}

/// The node being dragged, if any.
#[derive(Resource, Default)]
pub(crate) struct Dragging(Option<Gesture>);

/// One drag in progress.
struct Gesture {
    path: Vec<usize>,
    kind: Kind,
    /// `delay` or `duration` at drag start.
    base_secs: f32,
    /// The same, live: what a release commits.
    value_secs: f32,
}

/// The path a box entity
/// ([`TimelineAction`](moxie_ui::elements::TimelineAction) or
/// [`TimelineBlock`](moxie_ui::elements::TimelineBlock)) was built for.
#[derive(Component, Clone)]
pub(crate) struct BoxPath(pub(crate) Vec<usize>);

/// The same, for a path's
/// [`TimelineGap`](moxie_ui::elements::TimelineGap).
#[derive(Component, Clone)]
pub(crate) struct GapPath(pub(crate) Vec<usize>);

/// The path a [`TimelineLink`](moxie_ui::elements::TimelineLink) was
/// built for.
#[derive(Component, Clone)]
pub(crate) struct LinkPath(pub(crate) Vec<usize>);

/// Makes `handle` an edge: dragging it edits `path`'s `delay`
/// (`Kind::Move`) or `duration` (`Kind::Resize`).
pub(crate) fn edge<'r, 'u, 'a, E: Element<FynixHost>>(
    handle: &'r mut ElementMut<'u, 'a, FynixHost, E>,
    path: Vec<usize>,
    kind: Kind,
) -> &'r mut ElementMut<'u, 'a, FynixHost, E> {
    handle
        .insert(EntityCursor::System(SystemCursorIcon::EwResize))
        .observe(|mut press: On<Pointer<Press>>| {
            press.propagate(false);
        })
        .observe(|mut release: On<Pointer<Release>>| {
            release.propagate(false);
        })
        .observe(|mut click: On<Pointer<Click>>| {
            click.propagate(false);
        })
        .observe(
            move |mut start: On<Pointer<DragStart>>,
                  editor_scene: Res<EditorScene>,
                  mut dragging: ResMut<Dragging>| {
                start.propagate(false);
                if start.button != PointerButton::Primary {
                    return;
                }
                let Some(base_secs) =
                    base_seconds(&editor_scene, &path, kind)
                else {
                    return;
                };

                dragging.0 = Some(Gesture {
                    path: path.clone(),
                    kind,
                    base_secs,
                    value_secs: base_secs,
                });
            },
        )
        .observe(
            move |mut drag: On<Pointer<Drag>>,
                  scale: Res<UiScale>,
                  mut dragging: ResMut<Dragging>,
                  editor_scene: Res<EditorScene>,
                  folded: Res<BlockFoldState>,
                  view: Res<TimelineView>,
                  settings: Res<EditorSettings>,
                  boxes: Query<(&BoxPath, &mut Node)>,
                  gaps: Query<
                (&GapPath, &mut Node),
                Without<BoxPath>,
            >,
                  links: Query<
                (&LinkPath, &mut Node),
                (Without<BoxPath>, Without<GapPath>),
            >| {
                drag.propagate(false);
                let Some(gesture) = &mut dragging.0 else {
                    return;
                };
                let step = settings.min_duration().as_secs_f32();
                let dx_secs = (view
                    .secs_from_dx(drag.distance.x / scale.0)
                    / step)
                    .round()
                    * step;

                gesture.value_secs = match gesture.kind {
                    Kind::Delay => {
                        (gesture.base_secs + dx_secs).max(0.0)
                    }
                    Kind::Resize => {
                        (gesture.base_secs + dx_secs).max(step)
                    }
                };

                relayout(
                    &editor_scene,
                    &folded,
                    *view,
                    &gesture.path,
                    gesture.kind,
                    gesture.value_secs,
                    boxes,
                    gaps,
                    links,
                );
            },
        )
        .observe(
            move |mut end: On<Pointer<DragEnd>>,
                  mut dragging: ResMut<Dragging>,
                  mut tick: ResMut<RebuildTick>,
                  mut commands: Commands| {
                end.propagate(false);
                // The release can land off the handle, which leaves
                // its pressed highlight stuck. A rebuild respawns it.
                tick.bump();
                let Some(gesture) = dragging.0.take() else {
                    return;
                };
                if gesture.value_secs != gesture.base_secs {
                    commands.queue(move |world: &mut World| {
                        commit(
                            world,
                            &gesture.path,
                            gesture.kind,
                            gesture.value_secs,
                        );
                    });
                }
            },
        )
}

/// Drops the drag without committing, re-laying the untouched tree to
/// undo the preview.
fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut dragging: ResMut<Dragging>,
    editor_scene: Res<EditorScene>,
    folded: Res<BlockFoldState>,
    view: Res<TimelineView>,
    boxes: Query<(&BoxPath, &mut Node)>,
    gaps: Query<(&GapPath, &mut Node), Without<BoxPath>>,
    links: Query<
        (&LinkPath, &mut Node),
        (Without<BoxPath>, Without<GapPath>),
    >,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    relayout(
        &editor_scene,
        &folded,
        *view,
        &gesture.path,
        gesture.kind,
        gesture.base_secs,
        boxes,
        gaps,
        links,
    );
}

/// Lays out a scratch copy of the tree with `secs` applied to `kind`'s
/// edit and pushes the result onto the spawned entities by path.
fn relayout(
    editor_scene: &EditorScene,
    folded: &BlockFoldState,
    view: TimelineView,
    path: &[usize],
    kind: Kind,
    secs: f32,
    boxes: Query<(&BoxPath, &mut Node)>,
    gaps: Query<(&GapPath, &mut Node), Without<BoxPath>>,
    links: Query<
        (&LinkPath, &mut Node),
        (Without<BoxPath>, Without<GapPath>),
    >,
) {
    let mut animation = editor_scene.scene().0.animation.clone();
    let Some(node) = node_at_mut(&mut animation, path) else {
        return;
    };
    apply_edit(node, kind, secs);

    let layout =
        block_layout::layout(&animation, view, folded.paths());
    apply_layout(&layout, boxes, gaps, links);
}

/// Pushes `layout` onto the spawned box, gap and link entities by
/// path.
fn apply_layout(
    layout: &[Placed],
    mut boxes: Query<(&BoxPath, &mut Node)>,
    mut gaps: Query<(&GapPath, &mut Node), Without<BoxPath>>,
    mut links: Query<
        (&LinkPath, &mut Node),
        (Without<BoxPath>, Without<GapPath>),
    >,
) {
    for (box_path, mut node) in &mut boxes {
        let Some(placed) =
            layout.iter().find(|p| p.path == box_path.0)
        else {
            continue;
        };
        node.left = placed.left();
        node.top = placed.top();
        node.width = placed.width();
        node.height = px(placed.h);
    }

    // A gap the drag has since closed collapses to nothing rather
    // than show a stale width. One the drag opens where none existed
    // waits for the next real rebuild.
    for (gap_path, mut node) in &mut gaps {
        let Some(placed) =
            layout.iter().find(|p| p.path == gap_path.0)
        else {
            continue;
        };
        node.left = placed.gap_left();
        node.top = placed.top();
        node.width = placed.gap_width();
        node.height = px(placed.h);
    }

    // A link the drag has since dropped hides. One it newly creates
    // waits for the next real rebuild, like a gap.
    for (link_path, mut node) in &mut links {
        let rect = layout
            .iter()
            .find(|p| p.path == link_path.0)
            .and_then(Placed::link_rect);
        let Some([left, top, width, height]) = rect else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        node.left = left;
        node.top = top;
        node.width = width;
        node.height = height;
    }
}

/// `path`'s current `delay` (move) or `duration` (resize). `None` for
/// a resize on a block, or a dangling path.
fn base_seconds(
    editor_scene: &EditorScene,
    path: &[usize],
    kind: Kind,
) -> Option<f32> {
    let node = node_at(&editor_scene.scene().0.animation, path)?;
    match kind {
        Kind::Delay => Some(delay_secs(node)),
        Kind::Resize => duration_secs(node),
    }
}

fn delay_secs(node: &SceneNode<Backend>) -> f32 {
    match node {
        SceneNode::Block { delay, .. }
        | SceneNode::Action { delay, .. }
        | SceneNode::Draft { delay, .. } => {
            delay.unwrap_or_default().as_secs_f32()
        }
    }
}

fn duration_secs(node: &SceneNode<Backend>) -> Option<f32> {
    match node {
        SceneNode::Action { action, .. } => {
            Some(action.duration.as_secs_f32())
        }
        SceneNode::Draft { duration, .. } => {
            Some(duration.as_secs_f32())
        }
        SceneNode::Block { .. } => None,
    }
}

/// Writes the drag's result into the scene.
fn commit(world: &mut World, path: &[usize], kind: Kind, secs: f32) {
    let Some(mut editor_scene) =
        world.get_resource_mut::<EditorScene>()
    else {
        return;
    };
    let Some(node) =
        node_at_mut(&mut editor_scene.edit().animation, path)
    else {
        return;
    };
    apply_edit(node, kind, secs);
}

/// `kind`'s edit, applied in place to whichever field it names.
fn apply_edit(node: &mut SceneNode<Backend>, kind: Kind, secs: f32) {
    match kind {
        Kind::Delay => {
            let delay = match node {
                SceneNode::Block { delay, .. }
                | SceneNode::Action { delay, .. }
                | SceneNode::Draft { delay, .. } => delay,
            };
            *delay =
                (secs > 0.0).then(|| Duration::from_secs_f32(secs));
        }
        Kind::Resize => match node {
            SceneNode::Action { action, .. } => {
                action.duration = Duration::from_secs_f32(secs);
            }
            SceneNode::Draft { duration, .. } => {
                *duration = Duration::from_secs_f32(secs);
            }
            SceneNode::Block { .. } => {}
        },
    }
}
