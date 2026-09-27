//! The project's own assets, for the asset picker: its internal
//! assets, every file under the project's folder and its bookmarks
//! whose extension is a registered kind, and every mesh inside a glTF
//! file there.

use std::any::TypeId;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::asset::AssetPath;
use bevy::gltf::{Gltf, GltfAssetLabel, GltfMesh};
use bevy::prelude::*;
use moxie_asset::{
    ABSOLUTE_SOURCE, AssetChoice, AssetChoices, AssetRef, AssetTypes,
    InternalAssets,
};
use moxie_ui::asset_picker::RefreshAssetChoices;

use crate::{ProjectBookmarks, ProjectPath};

const INTERNAL_GROUP: &str = "Project";
const FILES_GROUP: &str = "Files";
/// How many folders deep a scan looks.
const MAX_DEPTH: usize = 6;
/// Where a scan stops, so a huge folder can't stall the editor.
const MAX_FILES: usize = 4096;
const GLTF_EXTENSIONS: &[&str] = &["glb", "gltf"];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Scanned>()
        .add_observer(on_refresh)
        .add_systems(
            Update,
            (
                on_gltf_loaded,
                // A new or renamed internal asset shows at once, with no
                // rescan of the disk.
                publish.run_if(resource_changed::<InternalAssets>),
            ),
        );
}

/// What the last scan found.
#[derive(Resource, Default)]
struct Scanned {
    /// Files of a registered kind.
    files: Vec<(TypeId, PathBuf)>,
    /// glTF files, held so their meshes stay listed.
    gltfs: Vec<(PathBuf, Handle<Gltf>)>,
}

fn on_refresh(_: On<RefreshAssetChoices>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        scan(world);
        publish(world);
    });
}

/// A glTF's meshes can only be listed once it has loaded.
fn on_gltf_loaded(
    mut events: MessageReader<AssetEvent<Gltf>>,
    mut commands: Commands,
) {
    let loaded = events.read().any(|event| {
        matches!(event, AssetEvent::LoadedWithDependencies { .. })
    });
    if loaded {
        commands.queue(publish);
    }
}

/// The project folder, then every bookmark.
fn folders(world: &World) -> Vec<PathBuf> {
    let project = world
        .resource::<ProjectPath>()
        .0
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    project
        .into_iter()
        .chain(world.resource::<ProjectBookmarks>().0.iter().cloned())
        .collect()
}

fn scan(world: &mut World) {
    let mut paths = Vec::new();
    for folder in folders(world) {
        walk(&folder, 0, &mut paths);
    }
    paths.sort();
    paths.dedup();

    let kinds = world.resource::<AssetTypes>();
    let files = paths
        .iter()
        .filter_map(|path| Some((kinds.kind_of(path)?, path.clone())))
        .collect::<Vec<_>>();

    let previous =
        std::mem::take(&mut world.resource_mut::<Scanned>().gltfs);
    let gltfs = paths
        .into_iter()
        .filter(|path| is_gltf(path))
        .map(|path| {
            let held = previous
                .iter()
                .find(|(seen, _)| *seen == path)
                .map(|(_, handle)| handle.clone());
            let handle = held.unwrap_or_else(|| {
                world
                    .resource::<AssetServer>()
                    .load_builder()
                    .override_unapproved()
                    .load(asset_path(&path))
            });
            (path, handle)
        })
        .collect();

    *world.resource_mut::<Scanned>() = Scanned { files, gltfs };
}

fn walk(folder: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let hidden = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn is_gltf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            GLTF_EXTENSIONS
                .contains(&extension.to_lowercase().as_str())
        })
}

/// A file's path through the absolute source, rooted at `/`.
fn asset_path(path: &Path) -> AssetPath<'static> {
    AssetPath::from_path_buf(path.to_path_buf())
        .with_source(ABSOLUTE_SOURCE)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Rewrites the found [`AssetChoices`] from the last scan, leaving it
/// untouched when nothing changed so a picker doesn't rebuild for
/// nothing.
fn publish(world: &mut World) {
    let mut found = HashMap::<TypeId, Vec<AssetChoice>>::new();
    for asset in world.resource::<InternalAssets>().iter() {
        found.entry(asset.kind).or_default().push(AssetChoice {
            name: asset.name.clone(),
            asset: AssetRef::Uuid(asset.id),
            group: INTERNAL_GROUP.to_string(),
        });
    }

    let choice = |name: String, path: AssetPath| AssetChoice {
        name,
        asset: AssetRef::Path(path.to_string()),
        group: FILES_GROUP.to_string(),
    };
    let scanned = world.resource::<Scanned>();
    for (kind, path) in &scanned.files {
        found
            .entry(*kind)
            .or_default()
            .push(choice(stem(path), asset_path(path)));
    }

    let gltfs = world.resource::<Assets<Gltf>>();
    let gltf_meshes = world.resource::<Assets<GltfMesh>>();
    let meshes = found.entry(TypeId::of::<Mesh>()).or_default();
    for (path, handle) in &scanned.gltfs {
        let Some(gltf) = gltfs.get(handle) else {
            continue;
        };
        let file = stem(path);
        let single = gltf.meshes.len() == 1;
        for mesh in gltf
            .meshes
            .iter()
            .filter_map(|mesh| gltf_meshes.get(mesh))
        {
            for primitive in &mesh.primitives {
                let mut name = if single {
                    file.clone()
                } else {
                    format!("{file}/{}", mesh.name)
                };
                if mesh.primitives.len() > 1 {
                    name = format!("{name}.{}", primitive.index);
                }
                let label = GltfAssetLabel::Primitive {
                    mesh: mesh.index,
                    primitive: primitive.index,
                };
                meshes.push(choice(
                    name,
                    asset_path(path).with_label(label.to_string()),
                ));
            }
        }
    }
    found.retain(|_, choices| !choices.is_empty());

    if !world.resource::<AssetChoices>().found_is(&found) {
        world.resource_mut::<AssetChoices>().set_found(found);
    }
}
