//! Saving and loading the editor's project file.
//!
//! The file format is [`moxie_asset::project`]'s. This is what the
//! editor does around it: picking the path, choosing what of the
//! world is saved, and swapping the loaded project in. The animation
//! addresses its subjects by [`EntityUid`], which only means anything
//! once the entities carrying those ids are back in the world.

use std::path::{Path, PathBuf};

use bevy::asset::AssetServer;
use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;
use bevy::world_serialization::{DynamicWorldBuilder, WorldFilter};
use bevy_motiongfx::scene::asset::MotionGfxScene;
use bevy_motiongfx::scene::backend::Backend;
use bevy_motiongfx::scene::id::EntityUid;
use motiongfx_scene::scene::Scene;
use moxie_asset::project::{
    EXTENSION, ProjectFile, ProjectRef, read_project, write_project,
};
use moxie_asset::{AnyPath, InternalAssets, replace_internal_assets};
use moxie_viewport::ViewportViews;

use crate::{
    EditorScene, ProjectBookmarks, ProjectLayout, ProjectPath,
    ProjectSettings, SceneRoot, SelectedAction, SelectedEntity,
    layout,
};

/// The project a blank one is: what it holds, how its dock is laid
/// out and where its viewport looks from. Saved from the editor.
const DEFAULT: &str = include_str!("../default.mox");

/// Replaces whatever is loaded with a blank project.
pub(crate) fn new_scene(world: &mut World) {
    if !load(world, DEFAULT, Path::new("default.mox")) {
        error!("the default project could not be read");
    }
    world.insert_resource(ProjectPath(None));
}

/// Spawns what a blank project starts with: a camera for each of 3D
/// and 2D and a light, so there is something to see the first
/// subject by.
pub(crate) fn spawn_defaults(world: &mut World) {
    let root = world
        .spawn((
            SceneRoot,
            Transform::IDENTITY,
            Visibility::Inherited,
        ))
        .id();
    spawn_stage(world, root);
}

/// A [`Camera`] that draws over the ones before it and clears
/// nothing.
pub(crate) fn overlay_camera() -> Camera {
    Camera {
        order: 1,
        clear_color: ClearColorConfig::None,
        ..default()
    }
}

/// The cameras and the light of a blank project, under `root`.
fn spawn_stage(world: &mut World, root: Entity) {
    world.spawn((
        EntityUid::new(),
        Name::new("Camera"),
        Camera3d::default(),
        Camera {
            clear_color: Color::srgb(0.02, 0.02, 0.04).into(),
            ..default()
        },
        Transform::from_xyz(0.0, 2.0, 14.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
        ChildOf(root),
    ));
    world.spawn((
        EntityUid::new(),
        Name::new("Camera 2D"),
        Camera2d,
        overlay_camera(),
        Transform::IDENTITY,
        Visibility::default(),
        ChildOf(root),
    ));
    world.spawn((
        EntityUid::new(),
        Name::new("Light"),
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 10.0, 5.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
        ChildOf(root),
    ));
}

/// Prompts for a path and writes the whole project to it.
pub(crate) fn save_scene(world: &mut World) {
    let Some(path) = ask_for_path(Dialog::Save) else {
        return;
    };

    let Some(text) = serialize(world, folder_of(&path)) else {
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

/// Replaces whatever is loaded with the project file at `path`, as
/// `File > Open` does once a path is picked.
pub fn open_path(world: &mut World, path: PathBuf) {
    match std::fs::read_to_string(&path) {
        Ok(text) => open(world, &text, path),
        Err(err) => {
            error!("could not read {}: {err}", path.display());
        }
    }
}

/// Replaces whatever is loaded with the project `text` holds, read
/// from `path`.
pub(crate) fn open(world: &mut World, text: &str, path: PathBuf) {
    if load(world, text, &path) {
        world.insert_resource(ProjectPath(Some(path)));
    }
}

/// Replaces whatever is loaded with the project `text` holds, whose
/// files are beside `path`. Whether it could be read.
fn load(world: &mut World, text: &str, path: &Path) -> bool {
    let Some(project) = deserialize(world, text, path) else {
        return false;
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
        return false;
    }
    stage_if_bare(world);
    layout::apply(world);
    moxie_viewport::restore_views(world);

    // The recompile runs on `EditorScene` changing, so inserting it
    // is the whole of loading the animation.
    world.insert_resource(EditorScene::new(MotionGfxScene(
        project.scene,
    )));
    world.insert_resource(ProjectBookmarks(project.bookmarks));
    true
}

/// Gives a project that came with no camera the cameras and the
/// light of a blank one, as one saved while the editor supplied them
/// does.
fn stage_if_bare(world: &mut World) {
    let filmed = world
        .query_filtered::<(), (With<Camera>, With<EntityUid>)>()
        .iter(world)
        .next()
        .is_some();
    if filmed {
        return;
    }
    let root = world
        .query_filtered::<Entity, With<SceneRoot>>()
        .iter(world)
        .next();
    match root {
        Some(root) => spawn_stage(world, root),
        // A file saved before subjects had a root.
        None => spawn_defaults(world),
    }
}

/// The folder a project file at `path` lives in.
fn folder_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("/"))
}

/// The project, as a file written into `folder`.
pub(crate) fn serialize(
    world: &mut World,
    folder: &Path,
) -> Option<String> {
    layout::capture(world);
    moxie_viewport::capture_views(world);
    // The root comes too, or the `ChildOf` on everything below it
    // would name an entity the file never held.
    let subjects: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<EntityUid>, With<SceneRoot>)>>()
        .iter(world)
        .collect();
    let world = &*world;

    // Before the registry is locked below: gathering them reads it
    // too, and a second read on a thread already holding one can
    // deadlock.
    let assets = world.resource::<InternalAssets>().to_save(world);

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let dynamic = DynamicWorldBuilder::from_world(world, &registry)
        .with_component_filter(subject_components())
        .extract_entities(subjects.into_iter())
        .deny_all_resources()
        .allow_resource::<ProjectSettings>()
        .allow_resource::<ProjectLayout>()
        .allow_resource::<ViewportViews>()
        .extract_resources()
        .build();

    let scene = world.resource::<EditorScene>();
    let bookmarks = world.resource::<ProjectBookmarks>();
    let project = ProjectRef {
        world: &dynamic,
        scene: &scene.scene().0,
        bookmarks: &bookmarks.0,
        assets: &assets,
        folder,
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
    let mut assets = AnyPath(world.resource::<AssetServer>());

    match read_project(
        text,
        &registry.read(),
        &mut assets,
        folder_of(path),
    ) {
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
/// hangs under it and bevy takes a despawned entity's descendants
/// with it. The selections go too: both name something in the scene
/// being replaced, and neither means anything in the one arriving.
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
    // A file saved without any keeps the defaults, not the last
    // project's.
    world.insert_resource(ProjectSettings::default());
    world.insert_resource(ProjectLayout::default());
    world.insert_resource(ViewportViews::default());
}

/// What a subject is saved as. An allowlist: the rest is the running
/// editor's business, and a file that hoards it would not load into
/// a different one.
fn subject_components() -> WorldFilter {
    WorldFilter::deny_all()
        .allow::<SceneRoot>()
        .allow::<EntityUid>()
        .allow::<Name>()
        .allow::<Transform>()
        .allow::<Visibility>()
        .allow::<Children>()
        .allow::<ChildOf>()
        .allow::<Camera>()
        .allow::<Camera3d>()
        .allow::<Projection>()
        .allow::<CascadeShadowConfig>()
        .allow::<DirectionalLight>()
        .allow::<PointLight>()
        .allow::<RectLight>()
        .allow::<SpotLight>()
        .allow::<Mesh3d>()
        .allow::<MeshMaterial3d<StandardMaterial>>()
        .allow::<Camera2d>()
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
