//! Saving and loading the editor's project file.
//!
//! The file format is [`moxie_asset::project`]'s. This is what the
//! editor does around it: picking the path, choosing what of the world
//! is saved, and swapping the loaded project in. The animation
//! addresses its subjects by [`EntityUid`], which only means anything
//! once the entities carrying those ids are back in the world.

use std::path::{Path, PathBuf};

use bevy::asset::AssetServer;
use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;
use bevy::reflect::{FromType, GetTypeRegistration, TypeRegistry};
use bevy::world_serialization::{DynamicWorldBuilder, WorldFilter};
use bevy_motiongfx::scene::asset::MotionGfxScene;
use bevy_motiongfx::scene::backend::Backend;
use bevy_motiongfx::scene::id::EntityUid;
use motiongfx_scene::scene::Scene;
use moxie_asset::project::{
    EXTENSION, ProjectFile, ProjectRef, read_project, write_project,
};
use moxie_asset::{InternalAssets, replace_internal_assets};

use crate::{
    EditorScene, ProjectBookmarks, ProjectPath, SceneRoot,
    SelectedAction, SelectedEntity,
};

/// Replaces whatever is loaded with a blank project.
pub(crate) fn new_scene(world: &mut World) {
    clear(world);
    replace_internal_assets(world, Vec::new());
    world.insert_resource(EditorScene::default());
    world.insert_resource(ProjectPath(None));
}

/// Prompts for a path and writes the whole project to it.
pub(crate) fn save_scene(world: &mut World) {
    let Some(path) = ask_for_path(Dialog::Save) else {
        return;
    };

    let Some(text) = serialize(world) else {
        return;
    };
    if let Err(err) = std::fs::write(&path, text) {
        error!("could not write {}: {err}", path.display());
        return;
    }
    world.insert_resource(ProjectPath(Some(path)));
}

/// Prompts for a path and replaces whatever is loaded with what it
/// holds.
pub(crate) fn load_scene(world: &mut World) {
    let Some(path) = ask_for_path(Dialog::Open) else {
        return;
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            error!("could not read {}: {err}", path.display());
            return;
        }
    };
    open(world, &text, path);
}

/// Replaces whatever is loaded with the project `text` holds, read
/// from `path`.
pub(crate) fn open(world: &mut World, text: &str, path: PathBuf) {
    let Some(project) = deserialize(world, text, &path) else {
        return;
    };

    clear(world);
    // Before the entities: nothing breaks if a handle outruns its
    // asset, but the first frame then draws nothing where it points.
    replace_internal_assets(world, project.assets);

    let registry = world.resource::<AppTypeRegistry>().clone();
    if let Err(err) = project.world.write_to_world_with(
        world,
        &mut default(),
        &registry.read(),
    ) {
        error!("could not spawn {}: {err}", path.display());
        return;
    }

    // The recompile runs on `EditorScene` changing, so inserting it is
    // the whole of loading the animation.
    world.insert_resource(EditorScene::new(MotionGfxScene(
        project.scene,
    )));
    world.insert_resource(ProjectBookmarks(project.bookmarks));
    world.insert_resource(ProjectPath(Some(path)));
}

pub(crate) fn serialize(world: &mut World) -> Option<String> {
    // The root comes too, or the `ChildOf` on everything below it
    // would name an entity the file never held.
    let subjects: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<EntityUid>, With<SceneRoot>)>>()
        .iter(world)
        .collect();
    let world = &*world;

    // Before the registry is locked below: gathering them reads it too,
    // and a second read on a thread already holding one can deadlock.
    let assets = world.resource::<InternalAssets>().to_save(world);

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let dynamic = DynamicWorldBuilder::from_world(world, &registry)
        .with_component_filter(subject_components(&registry))
        .extract_entities(subjects.into_iter())
        .build();

    let scene = world.resource::<EditorScene>();
    let bookmarks = world.resource::<ProjectBookmarks>();
    let project = ProjectRef {
        world: &dynamic,
        scene: &scene.scene().0,
        bookmarks: &bookmarks.0,
        assets: &assets,
    };
    match write_project(&project, &registry) {
        Ok(text) => Some(text),
        Err(err) => {
            error!("could not serialize the project: {err}");
            None
        }
    }
}

fn deserialize(
    world: &mut World,
    text: &str,
    path: &Path,
) -> Option<ProjectFile<Scene<Backend>>> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let mut assets = world.resource::<AssetServer>().clone();

    match read_project(text, &registry.read(), &mut assets) {
        Ok(project) => Some(project),
        Err(err) => {
            error!("could not read {}: {err}", path.display());
            None
        }
    }
}

/// Drops the loaded project, so nothing of it outlives the load.
///
/// Despawning [`SceneRoot`] is the whole of it, since every subject
/// hangs under it and bevy takes a despawned entity's descendants with
/// it. The selections go too: both name something in the scene being
/// replaced, and neither means anything in the one arriving.
fn clear(world: &mut World) {
    let roots = world
        .query_filtered::<Entity, With<SceneRoot>>()
        .iter(world)
        .collect::<Vec<_>>();

    if roots.len() > 1 {
        warn!("There is more that one root in the world");
    }

    for root in roots {
        if let Ok(entity) = world.get_entity_mut(root) {
            entity.despawn();
        }
    }

    world.insert_resource(SelectedEntity(None));
    world.insert_resource(SelectedAction(None));
    world.insert_resource(ProjectBookmarks::default());
}

pub(crate) fn plugin(app: &mut App) {
    app.save_component::<SceneRoot>()
        .save_component::<EntityUid>()
        .save_component::<Name>()
        .save_component::<Transform>()
        .save_component::<Visibility>()
        .save_component::<Children>()
        .save_component::<ChildOf>()
        .save_component::<Camera3d>()
        .save_component::<CascadeShadowConfig>()
        .save_component::<DirectionalLight>()
        .save_component::<PointLight>()
        .save_component::<RectLight>()
        .save_component::<SpotLight>()
        .save_component::<Mesh3d>()
        .save_component::<MeshMaterial3d<StandardMaterial>>()
        .save_component::<Camera2d>();
}

/// Marks a component a project file saves with its subject. The rest
/// is the running editor's business, and a file that hoards it would
/// not load into a different one.
#[derive(Clone, Copy)]
pub(crate) struct ReflectSaved;

impl<T> FromType<T> for ReflectSaved {
    fn from_type() -> Self {
        Self
    }
}

pub(crate) trait SaveAppExt {
    /// Saves `T` with every subject that has one.
    fn save_component<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
    ) -> &mut Self;
}

impl SaveAppExt for App {
    fn save_component<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
    ) -> &mut Self {
        self.register_type::<T>()
            .register_type_data::<T, ReflectSaved>()
    }
}

/// Every component marked [`ReflectSaved`], and nothing else.
fn subject_components(registry: &TypeRegistry) -> WorldFilter {
    registry.iter_with_data::<ReflectSaved>().fold(
        WorldFilter::deny_all(),
        |filter, (registration, _)| {
            filter.allow_by_id(registration.type_id())
        },
    )
}

enum Dialog {
    Open,
    Save,
}

/// Where a project file is picked, starting at the editor's own scene
/// folder. `None` when the dialog was dismissed.
fn ask_for_path(dialog: Dialog) -> Option<PathBuf> {
    let file = rfd::FileDialog::new()
        .add_filter("MotionGfx project", &[EXTENSION]);

    match dialog {
        Dialog::Open => file.pick_file(),
        // A dialog that types the extension for you still lets it be
        // left off, and the loader finds a file by it.
        Dialog::Save => file
            .save_file()
            .map(|path| path.with_extension(EXTENSION)),
    }
}
