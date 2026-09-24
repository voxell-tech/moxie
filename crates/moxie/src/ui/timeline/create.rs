//! Creating an action by dropping an animatable field from the
//! inspector onto the timeline.
//!
//! The pickup and the tag that follows the cursor are `moxie_ui`'s
//! generic field drag ([`DraggedField`]). This module is the timeline
//! half: the landing preview while a field is held over the track, and
//! on release splicing a fresh [`SceneNode::Action`] into the tree.
//! Drop resolution (merge / chain / plain insert) and the landing
//! hints are shared with [`reorder`].

use core::time::Duration;
use std::collections::BTreeSet;

use bevy::picking::events::{DragDrop, Pointer};
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_motiongfx::scene::backend::{AnimInterp, AnimOp, Backend};
use bevy_motiongfx::scene::id::{EntityUid, SceneUid};
use motiongfx_scene::block::{ActionCmd, Block, Node as SceneNode};
use motiongfx_scene::refs::FieldRef;
use motiongfx_scene::scene::{FieldSeed, Subject};
use moxie_ui::cursor::Cursor;
use moxie_ui::inspector::{DraggedField, Field};
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::{BevyFynix, FynixSet};

use super::hint::HideLanding;
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

/// Registers the hover preview and the drop.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, preview.after(FynixSet))
        .add_observer(on_drop);
}

/// Each frame a field is held: resolve where a release would land and
/// draw the same hints `reorder` uses.
fn preview(
    kernel: Res<BevyFynix>,
    pointer: Cursor,
    dragged: Res<DraggedField>,
    editor_scene: Res<EditorScene>,
    folded: Res<BlockFoldState>,
    view: Res<TimelineView>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    mut was_dragging: Local<bool>,
    mut commands: Commands,
) {
    if dragged.field.is_none() {
        // Gated on the edge: this branch runs on every frame nothing
        // is field-dragged, and `reorder` shares this hint for its
        // own.
        if *was_dragging {
            // The drag may have ended without a `DragDrop` over the
            // track (e.g. released elsewhere) - `on_drop` never ran
            // to hide the hint this hovering left showing.
            commands.trigger(HideLanding);
        }
        *was_dragging = false;
        return;
    }
    *was_dragging = true;
    let (
        Some(cursor),
        Ok((viewport_node, viewport_transform, scroll)),
    ) = (pointer.position(), q_viewport.single())
    else {
        commands.trigger(HideLanding);
        return;
    };
    let viewport_rect =
        logical_rect(viewport_node, viewport_transform);
    if !viewport_rect.contains(cursor) {
        commands.trigger(HideLanding);
        return;
    }

    let content = Vec2::new(
        cursor.x - viewport_rect.min.x,
        cursor.y - viewport_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(root, *view, folded.paths());
    let target = reorder::resolve(content, &layout, root, NO_NODE);

    reorder::announce_landing(
        &mut commands,
        kernel.theme(),
        target.as_ref(),
        &layout,
        root,
    );
}

/// On release over the track: build the action and splice it in.
///
/// Global, like [`reorder::on_drag_end`]: `Pointer<DragDrop>` fires on
/// whatever is under the cursor, and a child button would stop it
/// propagating from a per-entity observer.
fn on_drop(
    drop: On<Pointer<DragDrop>>,
    scale: Res<UiScale>,
    view: Res<TimelineView>,
    folded: Res<BlockFoldState>,
    mut dragged: ResMut<DraggedField>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    mut commands: Commands,
) {
    // DragEnd still runs and despawns the tag; taking it here stops a
    // second DragDrop this frame acting on the same drop.
    let Some(field) = dragged.field.take() else {
        return;
    };
    commands.trigger(HideLanding);
    let cursor = drop.pointer_location.position / scale.0;
    let Ok((viewport_node, viewport_transform, scroll)) =
        q_viewport.single()
    else {
        return;
    };
    let viewport_rect =
        logical_rect(viewport_node, viewport_transform);
    if !viewport_rect.contains(cursor) {
        return;
    }

    let content = Vec2::new(
        cursor.x - viewport_rect.min.x,
        cursor.y - viewport_rect.min.y + scroll.y,
    );
    let view = *view;
    let folded = folded.paths().clone();

    commands.queue(move |world: &mut World| {
        create(world, &field, content, view, &folded);
    });
}

/// Resolves the drop, captures the field's live value, and writes a
/// new [`SceneNode::Action`] into the tree.
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

        // Only the first action on a field needs its own seed; later
        // actions reuse it instead of orphaning a pool entry.
        let existing_seed = scene
            .0
            .stage
            .subjects
            .iter()
            .find(|s| s.id == SceneUid::Entity(uid))
            .and_then(|s| {
                s.fields
                    .iter()
                    .find(|seed| seed.field == field_ref)
                    .map(|seed| seed.value)
            });

        let seed_id = match existing_seed {
            Some(seed_id) => seed_id,
            None => {
                // Separate from `id`: the action panel edits an
                // action's value in place, so sharing one would let
                // editing it overwrite the stage too.
                let Some(seed_id) =
                    bevy_motiongfx::scene::value_pool::insert_scene_value(
                        &mut scene.values,
                        &type_registry.read(),
                        &*value,
                    )
                else {
                    return;
                };
                seed_id
            }
        };

        // The field's live value, at the moment nothing has animated
        // it yet - the only point this is also its correct staged
        // starting value. `stage` is a no-op without an entry here,
        // and baking silently falls back to whatever the world
        // already holds.
        seed_field(
            &mut scene.0.stage.subjects,
            SceneUid::Entity(uid),
            field_ref.clone(),
            seed_id,
        );

        let node = SceneNode::action(ActionCmd {
            subject: SceneUid::Entity(uid),
            field: field_ref,
            op: AnimOp::To,
            value: id,
            duration: DEFAULT_DURATION,
            ease: None,
            interp: Some(AnimInterp::Linear),
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

/// Stages `field` on `subject` at `value`, unless it already is -
/// only the first action ever created for a field needs one; every
/// later one's start comes from replaying what came before it.
fn seed_field(
    subjects: &mut Vec<Subject<Backend>>,
    subject: SceneUid,
    field: FieldRef,
    value: bevy::asset::uuid::Uuid,
) {
    let entry = subjects.iter_mut().find(|s| s.id == subject);
    match entry {
        Some(entry) => {
            if !entry.fields.iter().any(|seed| seed.field == field) {
                entry.fields.push(FieldSeed { field, value });
            }
        }
        None => subjects.push(Subject {
            id: subject,
            fields: vec![FieldSeed { field, value }],
        }),
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
