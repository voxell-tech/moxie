//! Creating an action by dropping an animatable field from the
//! inspector onto the timeline.
//!
//! The pickup and the tag that follows the cursor are `moxie_ui`'s
//! generic field drag ([`DraggedField`]). This module is the timeline
//! half: the landing preview while a field is held over the track,
//! and on release splicing a fresh [`SceneNode::Action`] into the
//! tree. Where it lands, and the hint marking it, are [`landing`]'s.

use core::time::Duration;
use std::collections::BTreeSet;

use bevy::asset::uuid::Uuid;
use bevy::picking::events::{DragDrop, Pointer};
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_fynix::Theme;
use bevy_motiongfx::scene::backend::{AnimInterp, AnimOp, Backend};
use bevy_motiongfx::scene::id::SceneUid;
use bevy_motiongfx::scene::value_pool::insert_scene_value;
use motiongfx_scene::block::{ActionCmd, Node as SceneNode};
use motiongfx_scene::refs::FieldRef;
use motiongfx_scene::scene::{FieldSeed, Scene, Subject};
use moxie_ui::cursor::{Cursor, PointerEventExt as _};
use moxie_ui::inspector::{DraggedField, Field};
use moxie_ui::layout::logical_rect;
use moxie_ui::theme::EditorTheme;

use super::hint::HintNode;
use super::{
    BlockFoldState, RebuildTick, TrackViewport, block_layout, landing,
};
use crate::{EditorScene, SelectedAction, TimelineView, subject};

/// A dropped field's action runs this long until there is a reason to
/// make it drag-configurable.
const DEFAULT_DURATION: Duration = Duration::from_secs(1);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, preview).add_observer(on_drop);
}

/// Each frame a field is held: resolve where a release would land and
/// draw its hint.
fn preview(
    theme: Res<Theme<EditorTheme>>,
    pointer: Cursor,
    hint: HintNode,
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
            hint.hide(&mut commands);
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
        hint.hide(&mut commands);
        return;
    };
    let viewport_rect =
        logical_rect(viewport_node, viewport_transform);
    if !viewport_rect.contains(cursor) {
        hint.hide(&mut commands);
        return;
    }

    let content = Vec2::new(
        cursor.x - viewport_rect.min.x,
        cursor.y - viewport_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(
        root,
        *view,
        folded.paths(),
        theme.0.space,
    );
    let target = landing::resolve(content, &layout, root, None);

    landing::announce_hint(
        &mut commands,
        &hint,
        &theme.0,
        target.as_ref(),
        &layout,
        root,
    );
}

/// On release over the track: build the action and splice it in.
fn on_drop(
    drop: On<Pointer<DragDrop>>,
    hint: HintNode,
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
    hint.hide(&mut commands);
    let cursor = drop.logical(&scale);
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

    let Some(subject::Target {
        subject,
        field: field_ref,
    }) = subject::Target::of(world, field)
    else {
        return;
    };
    let Some(value) = field.get(world) else {
        return;
    };
    let type_registry = world.resource::<AppTypeRegistry>().clone();

    let target = {
        let root =
            &world.resource::<EditorScene>().scene().0.animation;
        let space = world.resource::<Theme<EditorTheme>>().0.space;
        let layout = block_layout::layout(root, view, folded, space);
        landing::resolve(content, &layout, root, None)
    };

    let landed = {
        let registry = type_registry.read();
        let mut editor = world.resource_mut::<EditorScene>();
        let Scene {
            stage,
            animation,
            values,
            ..
        } = &mut editor.edit().0;
        let mut pool =
            || insert_scene_value(values, &registry, &*value);

        let Some(id) = pool() else {
            return;
        };
        // The field's live value, at the moment nothing has animated
        // it yet - the only point this is also its correct staged
        // starting value. `stage` is a no-op without an entry here,
        // and baking silently falls back to whatever the world
        // already holds. Pooled apart from `id`: the action panel
        // edits an action's value in place, and that must not move
        // the stage too.
        if seed_field(&mut stage.subjects, subject, &field_ref, pool)
            .is_none()
        {
            return;
        }

        let node = SceneNode::action(ActionCmd {
            subject,
            field: field_ref,
            op: AnimOp::To,
            value: id,
            duration: DEFAULT_DURATION,
            ease: None,
            interp: Some(AnimInterp::Linear),
            name: None,
        });
        match target {
            Some(target) => landing::place(animation, &target, node),
            // Loose past every block: a top-level child.
            None => {
                animation.children.push(node);
                Some(vec![animation.children.len() - 1])
            }
        }
    };

    if let Some(path) = landed
        && let Some(mut selected) =
            world.get_resource_mut::<SelectedAction>()
    {
        selected.0 = Some(path);
    }
    RebuildTick::bump_in(world);
}

/// Stages `field` on `subject` at what `pool` pools, unless it
/// already is: only the first action ever created for a field needs
/// one, and every later one's start comes from replaying what came
/// before it. `None` when `pool` fails.
fn seed_field(
    subjects: &mut Vec<Subject<Backend>>,
    subject: SceneUid,
    field: &FieldRef,
    pool: impl FnOnce() -> Option<Uuid>,
) -> Option<()> {
    let entry = subjects.iter_mut().find(|s| s.id == subject);
    if entry.as_ref().is_some_and(|s| {
        s.fields.iter().any(|seed| seed.field == *field)
    }) {
        return Some(());
    }
    let seed = FieldSeed {
        field: field.clone(),
        value: pool()?,
    };
    match entry {
        Some(entry) => entry.fields.push(seed),
        None => subjects.push(Subject {
            id: subject,
            fields: vec![seed],
        }),
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use bevy_motiongfx::scene::id::EntityUid;

    use super::*;

    #[test]
    fn a_field_is_seeded_once() {
        let subject = SceneUid::Entity(EntityUid::new());
        let field = FieldRef::new("T", ".x");
        let mut subjects = Vec::new();

        let first = Uuid::new_v4();
        seed_field(&mut subjects, subject, &field, || Some(first));
        seed_field(&mut subjects, subject, &field, || {
            panic!("a seeded field pools nothing")
        });

        let [entry] = subjects.as_slice() else {
            panic!("{} subjects, not one", subjects.len());
        };
        let [seed] = entry.fields.as_slice() else {
            panic!("{} seeds, not one", entry.fields.len());
        };
        assert_eq!(seed.value, first);
    }

    #[test]
    fn a_failed_pool_stages_nothing() {
        let subject = SceneUid::Entity(EntityUid::new());
        let field = FieldRef::new("T", ".x");
        let mut subjects = Vec::new();

        assert!(
            seed_field(&mut subjects, subject, &field, || None)
                .is_none()
        );
        assert!(subjects.is_empty());
    }
}
