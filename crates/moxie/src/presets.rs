//! Built-in assets offered wherever one is picked, as `(name, path)`
//! pairs relative to the workspace's `assets` folder.

use bevy::prelude::*;
use moxie_asset::AssetKindAppExt as _;

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

pub const MATERIALS: &[(&str, &str)] =
    &[("Default", "materials/default.mat")];

pub(crate) fn plugin(app: &mut App) {
    app.register_asset_choices::<Mesh>(MESHES)
        .register_asset_choices::<StandardMaterial>(MATERIALS);
}
