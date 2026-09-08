//! Creating an action by dropping an animatable field from the
//! inspector onto the timeline.
//!
//! The pickup and the tag that follows the cursor are `moxie_ui`'s
//! generic field drag ([`DraggedField`]). This module is the timeline
//! half: the landing preview while a field is held over the track, and
//! on release splicing a fresh [`Node::Action`] into the tree. Drop
//! resolution (merge / chain / plain insert) and the landing hints are
//! shared with [`reorder`](super::reorder).

use core::time::Duration;
use std::collections::BTreeSet;

use bevy::picking::events::{DragDrop, Pointer};
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_fynix::BevyFynix;
use bevy_motiongfx::scene::backend::{AnimOp, Backend};
use bevy_motiongfx::scene::id::{EntityUid, SceneUid};
use motiongfx_scene::block::{ActionCmd, Block, Node as SceneNode};
use moxie_ui::inspector::{DraggedField, Field};
use moxie_ui::layout::logical_rect;
use moxie_ui::theme::EditorTheme;

use super::reorder::{self, Target};
use super::{BlockFoldState, RebuildTick, TrackViewport};
use crate::block_layout;
use crate::ui::inspector::field_ref_of;
use crate::{EditorScene, SelectedAction, TimelineView};

/// A dropped field's action runs this long until there is a reason to
/// make it drag-configurable.
const DEFAULT_DURATION: Duration = Duration::from_secs(1);

/// Sentinel "no such node" path for [`reorder::resolve`], which filters
/// against the node being moved - a fresh node is under nothing.
const NO_NODE: &[usize] = &[usize::MAX];

/// The pointer in logical screen space, if it has a location.
fn cursor(
    pointers: &Query<&PointerLocation>,
    scale: &UiScale,
) -> Option<Vec2> {
    pointers
        .iter()
        .find_map(|pointer| pointer.location())
        .map(|location| location.position / scale.0)
}

/// Each frame a field is held: resolve where a release would land and
/// draw the same hints `reorder` uses.
pub(super) fn preview(
    kernel: Res<BevyFynix<EditorTheme>>,
    scale: Res<UiScale>,
    dragged: Res<DraggedField>,
    editor_scene: Res<EditorScene>,
    folded: Res<BlockFoldState>,
    visuals: Option<Res<reorder::Visuals>>,
    view: Res<TimelineView>,
    pointers: Query<&PointerLocation>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    q_area: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut nodes: Query<&mut Node>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
) {
    let Some(visuals) = visuals else {
        return;
    };
    let clear = |nodes: &mut Query<&mut Node>| {
        reorder::hide_landing(nodes, &visuals);
    };

    if dragged.field.is_none() {
        return;
    }
    let (Some(cursor), Ok((vp_node, vp_transform, scroll))) =
        (cursor(&pointers, &scale), q_viewport.single())
    else {
        clear(&mut nodes);
        return;
    };
    let Ok((area_node, area_transform)) = q_area.get(visuals.area())
    else {
        return;
    };
    let vp_rect = logical_rect(vp_node, vp_transform);
    if !vp_rect.contains(cursor) {
        clear(&mut nodes);
        return;
    }

    let content = Vec2::new(
        cursor.x - vp_rect.min.x,
        cursor.y - vp_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(root, *view, folded.paths());
    let target = reorder::resolve(content, &layout, root, NO_NODE);

    let area_rect = logical_rect(area_node, area_transform);
    let to_area = Vec2::new(
        vp_rect.min.x - area_rect.min.x,
        vp_rect.min.y - scroll.y - area_rect.min.y,
    );
    reorder::show_landing(
        &mut nodes,
        &mut backgrounds,
        &mut borders,
        &visuals,
        kernel.theme(),
        target.as_ref(),
        &layout,
        root,
        to_area,
    );
}

/// On release over the track: build the action and splice it in.
///
/// Global, like [`reorder::on_drag_end`]: `Pointer<DragDrop>` fires on
/// whatever is under the cursor, and a child button would stop it
/// propagating from a per-entity observer.
pub(super) fn on_drop(
    _: On<Pointer<DragDrop>>,
    scale: Res<UiScale>,
    view: Res<TimelineView>,
    folded: Res<BlockFoldState>,
    visuals: Option<Res<reorder::Visuals>>,
    mut dragged: ResMut<DraggedField>,
    pointers: Query<&PointerLocation>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    // DragEnd still runs and despawns the tag; taking it here stops a
    // second DragDrop this frame acting on the same drop.
    let Some(field) = dragged.field.take() else {
        return;
    };
    if let Some(visuals) = &visuals {
        reorder::hide_landing(&mut nodes, visuals);
    }
    let (Some(cursor), Ok((vp_node, vp_transform, scroll))) =
        (cursor(&pointers, &scale), q_viewport.single())
    else {
        return;
    };
    let vp_rect = logical_rect(vp_node, vp_transform);
    if !vp_rect.contains(cursor) {
        return;
    }

    let content = Vec2::new(
        cursor.x - vp_rect.min.x,
        cursor.y - vp_rect.min.y + scroll.y,
    );
    let view = *view;
    let folded = folded.paths().clone();

    commands.queue(move |world: &mut World| {
        create(world, &field, content, view, &folded);
    });
}

/// Resolves the drop, captures the field's live value, and writes a
/// new [`Node::Action`] into the tree.
fn create(
    world: &mut World,
    field: &Field,
    content: Vec2,
    view: TimelineView,
    folded: &BTreeSet<Vec<usize>>,
) {
    use moxie_ui::inspector::Source;

    let Some(uid) = world.get::<EntityUid>(field.entity()).copied()
    else {
        return;
    };
    let Some(field_ref) = field_ref_of(world, field) else {
        return;
    };
    let Some(value) = field.get(world) else {
        return;
    };
    let type_registry = world.resource::<AppTypeRegistry>().clone();

    let target = {
        let root =
            &world.resource::<EditorScene>().scene().0.animation;
        let layout = block_layout::layout(root, view, folded);
        reorder::resolve(content, &layout, root, NO_NODE)
    };

    let landed = {
        let mut editor = world.resource_mut::<EditorScene>();
        let scene = editor.edit();

        let Some(id) =
            bevy_motiongfx::scene::value_pool::insert_scene_value(
                &mut scene.values,
                &type_registry.read(),
                &*value,
            )
        else {
            return;
        };

        let node = SceneNode::action(ActionCmd {
            subject: SceneUid::Entity(uid),
            field: field_ref,
            op: AnimOp::To,
            value: id,
            duration: DEFAULT_DURATION,
            ease: None,
            interp: None,
            name: None,
        });
        splice(&mut scene.0.animation, target, node)
    };

    if let Some(path) = landed
        && let Some(mut selected) =
            world.get_resource_mut::<SelectedAction>()
    {
        selected.0 = Some(path);
    }
    if let Some(mut tick) = world.get_resource_mut::<RebuildTick>() {
        tick.0 = tick.0.wrapping_add(1);
    }
}

/// Puts `node` where `target` says, returning the path it landed at.
fn splice(
    root: &mut Block<Backend>,
    target: Option<Target>,
    node: SceneNode<Backend>,
) -> Option<Vec<usize>> {
    match target {
        // Loose past every block: a top-level child.
        None => {
            root.children.push(node);
            Some(vec![root.children.len() - 1])
        }
        Some(Target::Insert { parent, index }) => {
            let block = reorder::block_at_mut(root, &parent)?;
            let at = index.min(block.children.len());
            block.children.insert(at, node);
            let mut path = parent;
            path.push(at);
            Some(path)
        }
        Some(Target::Merge {
            path,
            combinator,
            before,
        }) => {
            let (&index, parent) = path.split_last()?;
            let block = reorder::block_at_mut(root, parent)?;
            if index >= block.children.len() {
                return None;
            }
            let host = block.children.remove(index);
            let children = if before {
                vec![node, host]
            } else {
                vec![host, node]
            };
            block.children.insert(
                index,
                SceneNode::block(Block {
                    combinator,
                    children,
                    name: None,
                }),
            );

            let mut landed = parent.to_vec();
            landed.push(index);
            landed.push(if before { 0 } else { 1 });
            Some(landed)
        }
    }
}
