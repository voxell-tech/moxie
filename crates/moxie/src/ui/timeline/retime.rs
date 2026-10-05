//! Retiming a node by dragging one of its box's edges: the left edge
//! edits `delay`, the right edge `duration` (leaves only - a block
//! has no `duration`). Dedicated handles leave the body free for
//! `reorder`'s merge gesture.
//!
//! Nothing writes [`EditorScene`] until [`DragEnd`]: the box list
//! watches it, so a mid-drag write would rebuild the dragged box out
//! from under the gesture. Each `Pointer<Drag>` instead lays out a
//! scratch copy of the tree with the tentative edit and pushes the
//! result onto the spawned entities by path. Escape re-lays the
//! untouched tree to undo the preview.

use core::time::Duration;

use bevy::input::ButtonInput;
use bevy::picking::events::{
    Click, Drag, DragEnd, DragStart, Pointer, Press, Release,
};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::SystemCursorIcon;
use bevy_fynix::{AnyView, Bevy, EntityCursor, Hovered, Theme, View};
use bevy_motiongfx::scene::backend::Backend;
use motiongfx_scene::block::Node as SceneNode;
use moxie_ui::elements::Selected;
use moxie_ui::theme::{EditorTheme, Spacing};

use super::super::action::{node_at, node_at_mut};
use super::block_layout::{self, Placed};
use super::{BlockFoldState, RebuildTick};
use crate::{EditorScene, ProjectSettings, TimelineView};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dragging>()
        .add_systems(Update, (cancel_on_escape, show_handles));
}

/// An edge handle's width.
pub(crate) const EDGE_HANDLE_PX: f32 = 6.0;
/// The width of an action's retime handle.
pub(crate) const ACTION_HANDLE_PX: f32 = 12.0;

/// On an action's retime handle, a child of the action's box.
#[derive(Component)]
pub(crate) struct ActionHandle;

/// Shows an action's handles while the action is hovered, selected
/// or being retimed. Hidden, they are not picked either.
fn show_handles(
    dragging: Res<Dragging>,
    boxes: Query<(&BoxPath, Has<Hovered>, Has<Selected>)>,
    mut handles: Query<
        (&ChildOf, &mut Visibility),
        With<ActionHandle>,
    >,
) {
    let retimed = dragging.0.as_ref().map(|gesture| &gesture.path);
    for (parent, mut visibility) in &mut handles {
        let shown = boxes.get(parent.parent()).is_ok_and(
            |(path, hovered, selected)| {
                hovered || selected || retimed == Some(&path.0)
            },
        );
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

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
    /// The time on the timeline at which that is zero: where the
    /// delay begins, or the node does.
    origin_secs: f32,
    /// The same, live: what a release commits.
    value_secs: f32,
}

/// The path a box entity (a `timeline_action` or `timeline_block`)
/// was built for.
#[derive(Component, Clone)]
pub(crate) struct BoxPath(pub(crate) Vec<usize>);

/// The same, for a path's `timeline_gap`.
#[derive(Component, Clone)]
pub(crate) struct GapPath(pub(crate) Vec<usize>);

/// The path a `timeline_link` was built for.
#[derive(Component, Clone)]
pub(crate) struct LinkPath(pub(crate) Vec<usize>);

/// `handle` as an edge: dragging it edits `path`'s `delay`
/// ([`Kind::Delay`]) or `duration` ([`Kind::Resize`]).
pub(crate) fn edge(
    handle: impl View<Bevy, EditorTheme> + 'static,
    path: Vec<usize>,
    kind: Kind,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::new(move |cx| {
        let node = cx.build(handle);
        wire_edge(cx.world, node, path, kind);
        node
    })
}

fn wire_edge(
    world: &mut World,
    node: Entity,
    path: Vec<usize>,
    kind: Kind,
) {
    world
        .entity_mut(node)
        .insert(EntityCursor(SystemCursorIcon::EwResize))
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
                  theme: Res<Theme<EditorTheme>>,
                  editor_scene: Res<EditorScene>,
                  folded: Res<BlockFoldState>,
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
                // A pixel to a second, so a box's place is its time.
                let begins = block_layout::layout(
                    &editor_scene.scene().0.animation,
                    TimelineView::UNIT,
                    folded.paths(),
                    theme.0.space,
                )
                .into_iter()
                .find(|placed| placed.path == path)
                .map_or(0.0, |placed| placed.x);
                let origin_secs = match kind {
                    Kind::Delay => begins - base_secs,
                    Kind::Resize => begins,
                };

                dragging.0 = Some(Gesture {
                    path: path.clone(),
                    kind,
                    base_secs,
                    origin_secs,
                    value_secs: base_secs,
                });
            },
        )
        .observe(
            move |mut drag: On<Pointer<Drag>>,
                  scale: Res<UiScale>,
                  theme: Res<Theme<EditorTheme>>,
                  mut dragging: ResMut<Dragging>,
                  editor_scene: Res<EditorScene>,
                  folded: Res<BlockFoldState>,
                  view: Res<TimelineView>,
                  settings: Res<ProjectSettings>,
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
                let step = settings.timestep().as_secs_f32();
                // The edge lands on the nearest timestep of the
                // timeline, wherever it started.
                let edge = gesture.origin_secs
                    + gesture.base_secs
                    + view.secs_from_dx(drag.distance.x / scale.0);
                let snapped = (edge / step).round() * step;
                let value = snapped - gesture.origin_secs;

                gesture.value_secs = match gesture.kind {
                    Kind::Delay => value.max(0.0),
                    Kind::Resize => value.max(step),
                };

                relayout(
                    &editor_scene,
                    &folded,
                    *view,
                    theme.0.space,
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
        );
}

/// Drops the drag without committing, re-laying the untouched tree to
/// undo the preview.
fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    theme: Res<Theme<EditorTheme>>,
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
        theme.0.space,
        &gesture.path,
        gesture.kind,
        gesture.base_secs,
        boxes,
        gaps,
        links,
    );
}

/// Lays out a scratch copy of the tree with `secs` applied to
/// `kind`'s edit and pushes the result onto the spawned entities by
/// path.
fn relayout(
    editor_scene: &EditorScene,
    folded: &BlockFoldState,
    view: TimelineView,
    space: Spacing,
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
        block_layout::layout(&animation, view, folded.paths(), space);
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

#[cfg(test)]
mod tests {
    use motiongfx_scene::block::{Block, Combinator};

    use super::*;

    fn draft(delay: Option<u64>) -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: delay.map(Duration::from_secs),
            duration: Duration::from_secs(2),
            name: None,
        }
    }

    #[test]
    fn a_delay_edit_sets_or_clears_the_delay() {
        let mut node = draft(None);
        apply_edit(&mut node, Kind::Delay, 1.5);
        assert_eq!(delay_secs(&node), 1.5);
        apply_edit(&mut node, Kind::Delay, 0.0);
        assert_eq!(delay_secs(&node), 0.0);
        let SceneNode::Draft { delay, .. } = node else {
            panic!("still a draft");
        };
        assert_eq!(delay, None);
    }

    #[test]
    fn a_resize_edit_sets_the_duration() {
        let mut node = draft(None);
        apply_edit(&mut node, Kind::Resize, 3.0);
        assert_eq!(duration_secs(&node), Some(3.0));
    }

    #[test]
    fn a_block_has_no_duration_to_resize() {
        let mut node = SceneNode::Block {
            delay: None,
            block: Block {
                combinator: Combinator::Chain,
                children: Vec::new(),
                name: None,
            },
        };
        apply_edit(&mut node, Kind::Resize, 3.0);
        assert_eq!(duration_secs(&node), None);
    }
}
