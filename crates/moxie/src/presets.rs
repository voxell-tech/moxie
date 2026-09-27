//! Built-in assets offered wherever one is picked: meshes as `(name,
//! path)` pairs relative to the workspace's `assets` folder, and a
//! default material made in code.

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use moxie_asset::{AssetKindAppExt as _, AssetRef};

const GROUP: &str = "Built-in";

pub const MESHES: &[(&str, &str)] = &[
    ("Cube", "meshes/cube.glb#Mesh0/Primitive0"),
    ("Plane", "meshes/plane.glb#Mesh0/Primitive0"),
    ("Sphere", "meshes/sphere.glb#Mesh0/Primitive0"),
    ("Sphere (Flat)", "meshes/sphere_flat.glb#Mesh0/Primitive0"),
    ("Icosphere", "meshes/icosphere.glb#Mesh0/Primitive0"),
    (
        "Icosphere (Flat)",
        "meshes/icosphere_flat.glb#Mesh0/Primitive0",
    ),
    ("Cylinder", "meshes/cylinder.glb#Mesh0/Primitive0"),
    (
        "Cylinder (Flat)",
        "meshes/cylinder_flat.glb#Mesh0/Primitive0",
    ),
    ("Cone", "meshes/cone.glb#Mesh0/Primitive0"),
    ("Cone (Flat)", "meshes/cone_flat.glb#Mesh0/Primitive0"),
    ("Torus", "meshes/torus.glb#Mesh0/Primitive0"),
    ("Torus (Flat)", "meshes/torus_flat.glb#Mesh0/Primitive0"),
    ("Monkey", "meshes/monkey.glb#Mesh0/Primitive0"),
];

/// A plain material, kept under a fixed id so a saved project names it
/// the same in every editor. Read-only: it is not an internal asset,
/// so nothing offers to edit it.
pub const DEFAULT_MATERIAL: Handle<StandardMaterial> =
    uuid_handle!("6f0c2e84-3b1d-4a5e-9c67-2d8f1a4b7e10");

pub(crate) fn plugin(app: &mut App) {
    let meshes = MESHES.iter().map(|(name, path)| {
        (name.to_string(), AssetRef::Path(path.to_string()))
    });
    let Handle::Uuid(default, _) = DEFAULT_MATERIAL else {
        unreachable!("uuid_handle! makes a Handle::Uuid");
    };

    app.register_asset_choices::<Mesh>(GROUP, meshes)
        .register_asset_choices::<StandardMaterial>(
            GROUP,
            [("Default".to_string(), AssetRef::Uuid(default))],
        )
        .add_systems(Startup, insert_default_material);
}

fn insert_default_material(
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A UUID id can't be stale, which is the only way this fails.
    let _ = materials
        .insert(&DEFAULT_MATERIAL, StandardMaterial::default());
}
