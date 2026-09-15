//! [`EditorScene`]: the editor's authoritative `Scene<Backend>`. It is
//! what gets edited (and, later, saved/loaded). The `Timeline` in
//! [`MotionGfxManager`] is a compiled, disposable view of it,
//! rebuilt by `recompile_dirty_scene` whenever it changes.

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;
use bevy_motiongfx::prelude::*;
use bevy_motiongfx::scene::asset::MotionGfxScene;
use bevy_motiongfx::scene::backend::{
    Backend, BackendRegistry, SceneRegistryExt,
    default_scene_registry,
};
use bevy_motiongfx::scene::value_pool::ValuePool;
use motiongfx_scene::block::{ActionCmd, Block, Node};
use motiongfx_scene::error::CompileError;
use motiongfx_scene::scene::{Scene, Stage};

/// The project: a scene plus the registry that resolves its names.
///
/// The action panel edits the tree, the timeline panel's row layout
/// reads it, and `recompile_dirty_scene` turns it back into a
/// timeline whenever `edit` lands a write.
///
/// Public (unlike most of this crate's state) because the example
/// binaries build it directly, in place of `motiongfx`'s imperative
/// track builder.
#[derive(Resource)]
pub struct EditorScene {
    scene: MotionGfxScene,
    registry: BackendRegistry,
    /// What [`scene_dirty`] diffs.
    version: SceneVersion,
}

/// What [`scene_dirty`] diffs: `generation` catches a whole
/// `EditorScene` being replaced, `edits` catches a write to it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SceneVersion {
    generation: u32,
    edits: u32,
}

impl EditorScene {
    pub fn new(scene: MotionGfxScene) -> Self {
        static NEXT_GENERATION: AtomicU32 = AtomicU32::new(0);
        let generation =
            NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);

        let mut registry = default_scene_registry();
        // Per-axis, so one axis of a translation or scale can be
        // animated on its own. Rotation stays whole-`Quat`: animating
        // one quaternion component denormalises it.
        registry
            .register_bundle(path!(<Transform>::translation::x))
            .register_bundle(path!(<Transform>::translation::y))
            .register_bundle(path!(<Transform>::translation::z))
            .register_bundle(path!(<Transform>::scale::x))
            .register_bundle(path!(<Transform>::scale::y))
            .register_bundle(path!(<Transform>::scale::z));

        Self {
            scene,
            registry,
            version: SceneVersion {
                generation,
                edits: 0,
            },
        }
    }

    pub(crate) fn scene(&self) -> &MotionGfxScene {
        &self.scene
    }

    /// The registry that resolves this scene's field, op, and interp
    /// names.
    pub(crate) fn registry(&self) -> &BackendRegistry {
        &self.registry
    }

    /// The scene, to change.
    pub(crate) fn edit(&mut self) -> &mut MotionGfxScene {
        self.version.edits = self.version.edits.wrapping_add(1);
        &mut self.scene
    }
}

/// Whether [`EditorScene::edit`] wrote, or the whole [`EditorScene`]
/// was replaced, since this last checked.
pub(crate) fn scene_dirty(
    scene: Res<EditorScene>,
    mut seen: Local<Option<SceneVersion>>,
) -> bool {
    let dirty = *seen != Some(scene.version);
    *seen = Some(scene.version);
    dirty
}

impl Default for EditorScene {
    /// Nothing in it, so a fresh editor already has something to edit
    /// and save before anything is loaded or created by hand.
    fn default() -> Self {
        Self::new(MotionGfxScene(Scene {
            stage: Stage {
                subjects: Vec::new(),
            },
            animation: Block::chain(Vec::new()),
            values: ValuePool::default(),
        }))
    }
}

/// Recompiles [`EditorScene`] into a fresh [`BevyTimeline`], replacing
/// whichever timeline the single player entity was showing while
/// preserving its playhead, or spawning that entity on the first
/// compile.
///
/// Scheduled with `run_if(scene_dirty)`, so this only runs when
/// [`EditorScene::edit`] landed a write.
pub(crate) fn recompile_dirty_scene(world: &mut World) {
    world.resource_scope::<EditorScene, _>(|world, mut editor_scene| {
        world.resource_scope::<MotionGfxManager, _>(
            |world, mut manager| {
                let mut q_players =
                    world.query::<(Entity, &TimelineId)>();
                let existing = q_players
                    .iter(world)
                    .next()
                    .map(|(entity, &id)| (entity, id));
                let playhead = existing.and_then(|(_, id)| {
                    manager
                        .get_timeline(&id)
                        .map(|timeline| timeline.target_time())
                });
                if let Some((_, old_id)) = existing {
                    manager.remove_timeline(&old_id);
                }

                let new_id = loop {
                    match editor_scene
                        .scene
                        .compile(&editor_scene.registry, &mut manager)
                    {
                        Ok(id) => break id,
                        Err(err) => {
                            if !demote_offending_action(
                                &mut editor_scene.scene.0.animation,
                                &err,
                            ) {
                                error!(
                                    "scene can't compile and no action could be demoted: {err}"
                                );
                                return;
                            }
                            warn!(
                                "demoted an action the scene can't compile: {err}"
                            );
                        }
                    }
                };

                editor_scene
                    .scene
                    .stage(&editor_scene.registry, world)
                    .expect("editor scene should stage");

                // Bakes the new timeline's `prev` off the world we
                // just staged, and replays up to the restored time -
                // otherwise the spawn pose `stage` wrote sits
                // unreplayed until the next scheduled sample, and
                // anything reading the world before then sees it raw.
                manager.load_pending_timelines(world);

                if let Some(time) = playhead
                    && let Some(timeline) =
                        manager.get_timeline_mut(&new_id)
                {
                    timeline.set_target_track(0);
                    timeline.set_target_time(time);
                }

                manager.sample_timelines(world);

                match existing {
                    Some((entity, _)) => {
                        world.entity_mut(entity).insert(new_id);
                    }
                    None => {
                        world.spawn((new_id, RealtimePlayer::new()));
                    }
                }
            },
        );
    });
}

/// Finds the first `Node::Action` under `block` that `error` names as
/// unresolvable, and turns it into a `Node::Draft` in place - keeping
/// its timing and name, dropping the subject/field/op/value `error`
/// says is broken. `true` if it found and demoted one.
fn demote_offending_action(
    block: &mut Block<Backend>,
    error: &CompileError<Backend>,
) -> bool {
    for child in &mut block.children {
        if let Node::Block { block, .. } = child {
            if demote_offending_action(block, error) {
                return true;
            }
            continue;
        }
        let Node::Action { delay, action } = child else {
            continue;
        };
        if !matches_error(action, error) {
            continue;
        }

        let (delay, duration, name) =
            (*delay, action.duration, action.name.clone());
        *child = Node::Draft {
            delay,
            duration,
            name,
        };
        return true;
    }

    false
}

fn matches_error(
    action: &ActionCmd<Backend>,
    error: &CompileError<Backend>,
) -> bool {
    match error {
        CompileError::UnknownSubject(id) => action.subject == *id,
        CompileError::UnknownField(field)
        | CompileError::UnknownSubjectKind(field)
        | CompileError::TypeMismatch { field, .. } => {
            action.field == *field
        }
        CompileError::UnknownOp(_, op) => action.op == *op,
        CompileError::UnknownValue(value) => action.value == *value,
        CompileError::UnknownEase(ease) => action.ease == Some(*ease),
        CompileError::UnknownInterp(interp) => {
            action.interp == Some(*interp)
        }
    }
}
